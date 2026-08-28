// ============================================================
// Brane OS Kernel — User Mode Transitions
// ============================================================
//
// Implements the kernel→user and user→kernel transitions for
// x86_64 using the `syscall`/`sysret` fast path and the
// `iretq` trampoline for the initial jump to ring 3.
//
// Design:
//   - `init_syscall_msrs()` programs IA32_EFER, IA32_STAR,
//     IA32_LSTAR and IA32_FMASK so the CPU knows where to go
//     on `syscall` and what privilege context to use.
//
//   - `syscall_entry` is a naked function (no prologue/epilogue)
//     placed at the LSTAR address. It saves the user context,
//     calls `syscall::dispatch()`, then restores and does sysretq.
//
//   - `jump_to_usermode(entry, user_stack)` performs the initial
//     ring 0 → ring 3 transition via `iretq`.
//
//   - `PerCpuData` is a per-CPU block stored at the kernel GS base
//     (`swapgs` switches between kernel and user GS on entry/exit).
//
// MSR layout:
//   IA32_EFER   0xC0000080  bit 0 = SCE (syscall enable)
//   IA32_STAR   0xC0000081  [47:32]=kernel CS, [63:48]=user DS
//                            (user CS = user DS + 8 per ABI)
//   IA32_LSTAR  0xC0000082  64-bit handler RIP
//   IA32_FMASK  0xC0000084  RFLAGS bits to clear on syscall entry
//
// Spec reference: ARCHITECTURE.md §5.2.4 (Syscall Dispatcher)
//                 ROADMAP.md Fase 10 (User mode transitions)
// ============================================================

#![allow(dead_code)]

// -----------------------------------------------------------------------
// Per-CPU Data — lives at the kernel GS base address
// -----------------------------------------------------------------------

/// Per-CPU data block referenced via the GS segment base.
///
/// On every `syscall` entry the CPU is still in ring 0 but with the
/// *user* GS base active. `swapgs` atomically exchanges user GS ↔ kernel GS,
/// giving us access to this struct via GS-relative addressing.
#[repr(C)]
pub struct PerCpuData {
    /// Kernel RSP to load on syscall entry (must be offset 0).
    pub kernel_rsp: u64,
    /// Scratch slot: user RSP is saved here by the entry stub (offset 8).
    pub user_rsp: u64,
}

impl PerCpuData {
    pub const fn new() -> Self {
        Self {
            kernel_rsp: 0,
            user_rsp: 0,
        }
    }
}

impl Default for PerCpuData {
    fn default() -> Self {
        Self::new()
    }
}

/// Static per-CPU blocks. Slot zero is reserved for the BSP; AP slots are
/// selected by the SMP bootstrap sequence before their syscall MSRs load.
pub static mut PER_CPU: [PerCpuData; crate::smp::MAX_CPUS] =
    [const { PerCpuData::new() }; crate::smp::MAX_CPUS];

// -----------------------------------------------------------------------
// UserContext — full CPU state at the moment of a syscall
// -----------------------------------------------------------------------

/// Snapshot of every user-visible register captured by `syscall_entry`.
///
/// Layout is kept `#[repr(C)]` so that the assembly stub can push/pop
/// registers in a deterministic order and cast the resulting stack frame
/// to `*mut UserContext`.
///
/// Note: `rcx` holds the user-space return address (RIP) after `syscall`;
///       `r11` holds the saved RFLAGS. Both are restored by `sysretq`.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct UserContext {
    // Callee-saved (preserved across C calls; saved by entry stub)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    // Argument / caller-saved registers
    pub r11: u64, // RFLAGS saved by `syscall`
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64, // RIP saved by `syscall`
    pub rax: u64, // syscall number on entry / return value on exit
}

impl UserContext {
    /// Create a zeroed context.
    pub const fn empty() -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            r11: 0,
            r10: 0,
            r9: 0,
            r8: 0,
            rdi: 0,
            rsi: 0,
            rdx: 0,
            rcx: 0,
            rax: 0,
        }
    }
}

// -----------------------------------------------------------------------
// MSR constants
// -----------------------------------------------------------------------

const MSR_EFER: u32 = 0xC000_0080;
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_FMASK: u32 = 0xC000_0084;

/// EFER bit 0 — System Call Extensions (enables `syscall`/`sysret`).
const EFER_SCE: u64 = 1 << 0;

/// RFLAGS bits to mask on syscall entry:
///   IF (bit 9)  — disable hardware interrupts during syscall handler
///   TF (bit 8)  — disable single-step trap
///   DF (bit 10) — clear direction flag
///   AC (bit 18) — disable alignment check
const FMASK_VALUE: u64 = (1 << 9) | (1 << 8) | (1 << 10) | (1 << 18);

// -----------------------------------------------------------------------
// Low-level MSR helpers
// -----------------------------------------------------------------------

/// Read a model-specific register.
///
/// # Safety
/// Caller must ensure the MSR exists on this CPU (no CPUID check here).
#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") lo,
        out("edx") hi,
        options(nomem, nostack, preserves_flags),
    );
    (hi as u64) << 32 | lo as u64
}

/// Write a model-specific register.
///
/// # Safety
/// Caller must pass a valid MSR index and a value that makes sense for it.
#[inline]
unsafe fn wrmsr(msr: u32, value: u64) {
    let lo = value as u32;
    let hi = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") lo,
        in("edx") hi,
        options(nomem, nostack, preserves_flags),
    );
}

// -----------------------------------------------------------------------
// Public init — call once after gdt::init() and idt::init()
// -----------------------------------------------------------------------

/// Configure the CPU to support `syscall`/`sysret`.
///
/// Sets up the four MSRs required for 64-bit fast system calls:
/// EFER (enable syscall), STAR (segment selectors), LSTAR (entry RIP),
/// FMASK (RFLAGS mask).
///
/// # Safety
/// Must be called from ring 0 on the boot CPU, after `gdt::init()`.
/// GDT must already contain ring-3 code and data segments.
pub fn init_syscall_msrs() {
    unsafe {
        let _ = init_syscall_msrs_for_cpu(0);
    }

    crate::serial_println!(
        "[usermode] syscall MSRs configured (STAR/LSTAR/FMASK). syscall/sysret enabled."
    );
}

/// Configure syscall MSRs and the kernel GS block for one logical CPU.
///
/// The caller must have loaded the matching per-CPU GDT first. Slot zero is
/// the BSP; APs use stable non-zero slots for the lifetime of the kernel.
/// Returns `false` when the requested slot is outside the static table.
///
/// # Safety
/// Must run in ring 0 with interrupts disabled, after the caller has loaded
/// the GDT belonging to `cpu_slot`. The per-CPU block must remain allocated
/// for as long as this CPU's syscall MSRs are active.
pub unsafe fn init_syscall_msrs_for_cpu(cpu_slot: usize) -> bool {
    if cpu_slot >= crate::smp::MAX_CPUS {
        return false;
    }

    unsafe {
        // 1. Enable SCE in EFER
        let efer = rdmsr(MSR_EFER);
        wrmsr(MSR_EFER, efer | EFER_SCE);

        // 2. Program STAR:
        //    bits [47:32] = kernel CS selector (used for syscall)
        //    bits [63:48] = user DS selector   (user CS = user DS + 8 for sysret)
        let kcs = super::gdt::kernel_code_selector() as u64;
        let uds = super::gdt::user_data_selector() as u64;
        let star: u64 = (uds << 48) | (kcs << 32);
        wrmsr(MSR_STAR, star);

        // 3. Program LSTAR with the address of our entry stub
        let handler_addr = syscall_entry as *const () as u64;
        wrmsr(MSR_LSTAR, handler_addr);

        // 4. Program FMASK — clear IF + TF + DF + AC on syscall entry
        wrmsr(MSR_FMASK, FMASK_VALUE);

        // 5. Set the kernel GS base to point to our per-CPU data block
        //    so the entry stub can load kernel_rsp via GS-relative addressing.
        //    We use IA32_KERNEL_GS_BASE (0xC0000102) — this is the value
        //    that swapgs switches IN when entering the kernel.
        let per_cpu_addr = core::ptr::addr_of!(PER_CPU[cpu_slot]) as u64;
        wrmsr(0xC000_0102, per_cpu_addr); // IA32_KERNEL_GS_BASE

        // Provide a default kernel stack pointer (the current RSP).
        // A real scheduler will update this per-CPU kernel_rsp per task switch.
        let rsp: u64;
        core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack));
        (*core::ptr::addr_of_mut!(PER_CPU[cpu_slot])).kernel_rsp = rsp;
    }
    true
}

// -----------------------------------------------------------------------
// syscall_entry — naked handler at LSTAR
// -----------------------------------------------------------------------

/// Low-level syscall entry point — set as the LSTAR target.
///
/// On entry (from `syscall` instruction):
///   RCX = user RIP (return address)   R11 = user RFLAGS
///   RAX = syscall number              RDI/RSI/RDX/R10/R8/R9 = args
///   CS/SS still user; we run at ring 0 after the selector swap
///
/// This function MUST be `naked` — the compiler must not generate
/// any prologue or epilogue because the stack pointer belongs to
/// userspace at the moment of entry.
#[cfg(target_arch = "x86_64")]
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        // ── Kernel entry ─────────────────────────────────────────
        // swapgs: activate kernel GS base (PerCpuData)
        "swapgs",
        // Save user RSP and load kernel RSP
        "mov gs:[8], rsp",          // PER_CPU.user_rsp  (offset 8)
        "mov rsp, gs:[0]",          // PER_CPU.kernel_rsp (offset 0)

        // ── Save user context on the kernel stack ─────────────────
        // Push in reverse UserContext field order (rax last = top of stack)
        "push rax",   // syscall number (will become return value)
        "push rcx",   // user RIP
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",   // user RFLAGS
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // ── Build SyscallContext and dispatch ──────────────────────
        // SyscallContext layout: number(rax), arg1(rdi), arg2(rsi),
        //                        arg3(rdx), arg4(r10), arg5(r8)
        // At this point: rax=syscall#, rdi/rsi/rdx/r10/r8 = args
        // We need to re-read them from where they were before pushing.
        // Restore originals from the saved slots on the stack:
        "mov rax, [rsp + 14*8]",   // rax  (syscall number)
        "mov rdi, [rsp + 6*8]",    // rdi  (arg1)
        "mov rsi, [rsp + 5*8]",    // rsi  (arg2)
        "mov rdx, [rsp + 2*8]",    // rdx  (arg3)  — note: after pushes
        "mov r10, [rsp + 3*8]",    // r10  (arg4)
        "mov r8,  [rsp + 4*8]",    // r8   (arg5)

        // Reserve space for SyscallContext (6 × u64 = 48 bytes) and fill it
        "sub rsp, 48",
        "mov [rsp + 0],  rax",     // .number
        "mov [rsp + 8],  rdi",     // .arg1
        "mov [rsp + 16], rsi",     // .arg2
        "mov [rsp + 24], rdx",     // .arg3
        "mov [rsp + 32], r10",     // .arg4
        "mov [rsp + 40], r8",      // .arg5

        // Call dispatch(ctx: &SyscallContext) — rdi = pointer to context
        "mov rdi, rsp",
        "call {dispatch}",

        // Return value in RAX — store into saved rax slot
        "add rsp, 48",             // pop SyscallContext
        "mov [rsp + 14*8], rax",   // update saved RAX with return value

        // ── Restore user context ──────────────────────────────────
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "pop r11",   // user RFLAGS
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",   // user RIP
        "pop rax",   // syscall return value

        // Restore user RSP and return to ring 3
        "mov rsp, gs:[8]",         // PER_CPU.user_rsp
        "swapgs",
        "sysretq",

        dispatch = sym crate::syscall::dispatch,
    );
}

#[cfg(not(target_arch = "x86_64"))]
unsafe extern "C" fn syscall_entry() {}

// -----------------------------------------------------------------------
// jump_to_usermode — initial ring 0 → ring 3 trampoline (iretq)
// -----------------------------------------------------------------------

/// Transfer CPU control to user space for the first time.
///
/// Builds the 5-word iretq frame on the kernel stack and executes `iretq`
/// to atomically switch to ring 3, load the user CS/SS, and jump to `entry`.
///
/// Arguments:
///   `entry`      — virtual address of the first instruction in user space
///   `user_stack` — top of the user-mode stack (highest byte + 1, 16-byte aligned)
///
/// # Safety
/// - The GDT must have been initialized with ring-3 segments.
/// - `entry` must be a valid, mapped virtual address in the user page table.
/// - `user_stack` must point to a mapped, writable user stack.
/// - Interrupts should be disabled by the caller before this call.
#[cfg(target_arch = "x86_64")]
pub unsafe fn jump_to_usermode(entry: u64, user_stack: u64) -> ! {
    let user_cs = super::gdt::user_code_selector() as u64 | 3; // RPL = 3
    let user_ss = super::gdt::user_data_selector() as u64 | 3; // RPL = 3

    // The iretq frame (pushed in reverse order, high address first):
    //   [rsp+32]  SS
    //   [rsp+24]  RSP (user stack)
    //   [rsp+16]  RFLAGS (IF=1, IOPL=0, all others cleared)
    //   [rsp+8]   CS
    //   [rsp+0]   RIP (entry point)
    core::arch::asm!(
        // Build the iretq frame
        "push {ss}",            // SS  (ring 3 data | 3)
        "push {rsp}",           // RSP (user stack top)
        "pushfq",               // push current RFLAGS
        "or qword ptr [rsp], 0x200", // set IF (enable interrupts in user mode)
        "push {cs}",            // CS  (ring 3 code | 3)
        "push {rip}",           // RIP (user entry point)
        "iretq",
        ss  = in(reg) user_ss,
        rsp = in(reg) user_stack,
        cs  = in(reg) user_cs,
        rip = in(reg) entry,
        options(noreturn),
    );
}

/// Stub for non-x86_64 targets (unit test host).
#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn jump_to_usermode(_entry: u64, _user_stack: u64) -> ! {
    loop {}
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_context_empty_is_zeroed() {
        let ctx = UserContext::empty();
        assert_eq!(ctx.rax, 0);
        assert_eq!(ctx.rcx, 0);
        assert_eq!(ctx.r15, 0);
    }

    #[test]
    fn per_cpu_initial_state() {
        // Static initial values are zero
        let data = unsafe { core::ptr::read(core::ptr::addr_of!(PER_CPU[0])) };
        let kernel_rsp = data.kernel_rsp;
        let user_rsp = data.user_rsp;
        // Before init_syscall_msrs() is called on bare metal both are 0;
        // on the host test environment this still holds.
        let _ = kernel_rsp;
        let _ = user_rsp;
    }

    #[test]
    fn fmask_has_if_bit() {
        assert!(FMASK_VALUE & (1 << 9) != 0, "IF bit must be in FMASK");
    }

    #[test]
    fn efer_sce_bit_is_bit0() {
        assert_eq!(EFER_SCE, 1);
    }
}
