// ============================================================
// Brane OS Kernel — ACPI Power Management
// ============================================================
//
// Implements parsing of ACPI tables to retrieve PM1a/b control block
// addresses, SLP_TYPa/b values for S5, and reset register details
// to support system shutdown and reboot.
//
// Spec reference: docs/ROADMAP.md Phase 10
// ============================================================

extern crate alloc;

use alloc::vec::Vec;
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::VirtAddr;

/// Generic Address Structure (GAS) as defined by ACPI
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct GenericAddressStructure {
    pub address_space: u8, // 0 = System Memory (MMIO), 1 = System I/O (Ports)
    pub bit_width: u8,
    pub bit_offset: u8,
    pub access_size: u8,
    pub address: u64,
}

#[repr(C, packed)]
struct RsdpHeader {
    signature: [u8; 8], // "RSD PTR "
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    // ACPI 2.0+ fields:
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3],
}

#[repr(C, packed)]
struct DescriptionHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u8; 4],
    creator_revision: u32,
}

#[repr(C, packed)]
struct Fadt {
    header: DescriptionHeader,
    firmware_ctrl: u32,
    dsdt: u32,
    reserved: u8,
    preferred_pm_profile: u8,
    sci_int: u16,
    smi_cmd: u32,
    acpi_enable: u8,
    acpi_disable: u8,
    s4bios_req: u8,
    pstate_cnt: u8,
    pm1a_evt_blk: u32,
    pm1b_evt_blk: u32,
    pm1a_cnt_blk: u32,
    pm1b_cnt_blk: u32,
    pm2_cnt_blk: u32,
    pm_tmr_blk: u32,
    gpe0_blk: u32,
    gpe1_blk: u32,
    pm1_evt_len: u8,
    pm1_cnt_len: u8,
    pm2_cnt_len: u8,
    pm_tmr_len: u8,
    gpe0_len: u8,
    gpe1_len: u8,
    gpe1_base: u8,
    cst_cnt: u8,
    p_lvl2_lat: u16,
    p_lvl3_lat: u16,
    flush_size: u16,
    flush_stride: u16,
    duty_offset: u8,
    duty_width: u8,
    day_alrm: u8,
    mon_alrm: u8,
    century: u8,
    iapc_boot_arch: u16,
    reserved2: u8,
    flags: u32,
    // ACPI 2.0+ fields:
    reset_reg: GenericAddressStructure,
    reset_value: u8,
    arm_boot_arch: u16,
    fadt_minor_version: u8,
    x_firmware_ctrl: u64,
    x_dsdt: u64,
    x_pm1a_evt_blk: GenericAddressStructure,
    x_pm1b_evt_blk: GenericAddressStructure,
    x_pm1a_cnt_blk: GenericAddressStructure,
    x_pm1b_cnt_blk: GenericAddressStructure,
    x_pm2_cnt_blk: GenericAddressStructure,
    x_pm_tmr_blk: GenericAddressStructure,
    x_gpe0_blk: GenericAddressStructure,
    x_gpe1_blk: GenericAddressStructure,
}

#[derive(Debug, Default)]
struct AcpiState {
    pm1a_cnt_blk: Option<u32>,
    pm1b_cnt_blk: Option<u32>,
    slp_typa: Option<u8>,
    slp_typb: Option<u8>,
    is_io: bool,
    reset_reg: Option<GenericAddressStructure>,
    reset_value: Option<u8>,
    rsdp_addr: Option<u64>,
    phys_mem_offset: Option<u64>,
}

static ACPI_STATE: Mutex<AcpiState> = Mutex::new(AcpiState {
    pm1a_cnt_blk: None,
    pm1b_cnt_blk: None,
    slp_typa: None,
    slp_typb: None,
    is_io: true,
    reset_reg: None,
    reset_value: None,
    rsdp_addr: None,
    phys_mem_offset: None,
});

/// Verify table checksum by adding all bytes (must sum to 0 modulo 256)
fn verify_checksum(addr: *const u8, len: usize) -> bool {
    let mut sum: u8 = 0;
    for i in 0..len {
        sum = sum.wrapping_add(unsafe { *addr.add(i) });
    }
    sum == 0
}

/// Initialize the ACPI module using the RSDP physical address and physical memory offset.
pub fn init(rsdp_phys_addr: u64, physical_memory_offset: u64) {
    let rsdp_virt = physical_memory_offset + rsdp_phys_addr;
    let rsdp = unsafe { &*(rsdp_virt as *const RsdpHeader) };

    // Check RSDP signature "RSD PTR "
    if &rsdp.signature != b"RSD PTR " {
        crate::serial_println!("[acpi] Error: Invalid RSDP signature.");
        return;
    }

    // Verify first revision checksum (20 bytes)
    if !verify_checksum(rsdp_virt as *const u8, 20) {
        crate::serial_println!("[acpi] Error: RSDP base checksum mismatch.");
        return;
    }

    // Determine whether to use XSDT (ACPI 2.0+) or RSDT (ACPI 1.0)
    let use_xsdt = rsdp.revision >= 2 && rsdp.xsdt_address != 0;
    let mut fadt_opt: Option<&'static Fadt> = None;

    if use_xsdt {
        let xsdt_virt = physical_memory_offset + rsdp.xsdt_address;
        let xsdt = unsafe { &*(xsdt_virt as *const DescriptionHeader) };
        if &xsdt.signature == b"XSDT" && verify_checksum(xsdt_virt as *const u8, xsdt.length as usize) {
            let entries_count = (xsdt.length as usize - 36) / 8;
            let entries_ptr = (xsdt_virt + 36) as *const u64;
            for i in 0..entries_count {
                let entry_phys = unsafe { *entries_ptr.add(i) };
                let entry_virt = physical_memory_offset + entry_phys;
                let header = unsafe { &*(entry_virt as *const DescriptionHeader) };
                if &header.signature == b"FACP" && verify_checksum(entry_virt as *const u8, header.length as usize) {
                    fadt_opt = Some(unsafe { &*(entry_virt as *const Fadt) });
                    break;
                }
            }
        }
    }

    if fadt_opt.is_none() {
        // Fall back to RSDT
        let rsdt_virt = physical_memory_offset + rsdp.rsdt_address as u64;
        let rsdt = unsafe { &*(rsdt_virt as *const DescriptionHeader) };
        if &rsdt.signature == b"RSDT" && verify_checksum(rsdt_virt as *const u8, rsdt.length as usize) {
            let entries_count = (rsdt.length as usize - 36) / 4;
            let entries_ptr = (rsdt_virt + 36) as *const u32;
            for i in 0..entries_count {
                let entry_phys = unsafe { *entries_ptr.add(i) } as u64;
                let entry_virt = physical_memory_offset + entry_phys;
                let header = unsafe { &*(entry_virt as *const DescriptionHeader) };
                if &header.signature == b"FACP" && verify_checksum(entry_virt as *const u8, header.length as usize) {
                    fadt_opt = Some(unsafe { &*(entry_virt as *const Fadt) });
                    break;
                }
            }
        }
    }

    let fadt = match fadt_opt {
        Some(f) => f,
        None => {
            crate::serial_println!("[acpi] Error: FADT (FACP) table not found.");
            // Store minimal info for fallback
            let mut state = ACPI_STATE.lock();
            state.rsdp_addr = Some(rsdp_phys_addr);
            state.phys_mem_offset = Some(physical_memory_offset);
            return;
        }
    };

    // Extract PM1a/b control block ports/addresses
    let mut pm1a_cnt = fadt.pm1a_cnt_blk;
    let mut pm1b_cnt = fadt.pm1b_cnt_blk;
    let mut is_io = true;

    // FADT length >= 148 contains ACPI 2.0+ Generic Address Structures for PM1a/b
    if fadt.header.length >= 148 {
        if fadt.x_pm1a_cnt_blk.address != 0 {
            pm1a_cnt = fadt.x_pm1a_cnt_blk.address as u32;
            pm1b_cnt = fadt.x_pm1b_cnt_blk.address as u32;
            is_io = fadt.x_pm1a_cnt_blk.address_space == 1;
        }
    }

    // Resolve DSDT physical address
    let dsdt_phys = if fadt.header.length >= 148 && fadt.x_dsdt != 0 {
        fadt.x_dsdt
    } else {
        fadt.dsdt as u64;
    };

    // Parse DSDT to find S5 sleep type values
    let mut slp_typa = None;
    let mut slp_typb = None;

    if dsdt_phys != 0 {
        let dsdt_virt = physical_memory_offset + dsdt_phys;
        let dsdt = unsafe { &*(dsdt_virt as *const DescriptionHeader) };
        if &dsdt.signature == b"DSDT" && verify_checksum(dsdt_virt as *const u8, dsdt.length as usize) {
            let aml_len = dsdt.length as usize - 36;
            let aml_ptr = (dsdt_virt + 36) as *const u8;
            let aml_slice = unsafe { core::slice::from_raw_parts(aml_ptr, aml_len) };
            if let Some((typa, typb)) = scan_s5_values(aml_slice) {
                slp_typa = Some(typa);
                slp_typb = Some(typb);
            }
        }
    }

    // Capture ACPI reboot reset register if supported
    let mut reset_reg = None;
    let mut reset_value = None;
    if fadt.header.length >= 129 {
        // RESET_REG_SUP is bit 10 in FADT flags
        let reset_supported = (fadt.flags & (1 << 10)) != 0;
        if reset_supported && fadt.reset_reg.address != 0 {
            reset_reg = Some(fadt.reset_reg);
            reset_value = Some(fadt.reset_value);
        }
    }

    let mut state = ACPI_STATE.lock();
    state.pm1a_cnt_blk = Some(pm1a_cnt);
    if pm1b_cnt != 0 {
        state.pm1b_cnt_blk = Some(pm1b_cnt);
    }
    state.slp_typa = slp_typa;
    state.slp_typb = slp_typb;
    state.is_io = is_io;
    state.reset_reg = reset_reg;
    state.reset_value = reset_value;
    state.rsdp_addr = Some(rsdp_phys_addr);
    state.phys_mem_offset = Some(physical_memory_offset);

    crate::serial_println!(
        "[acpi] ACPI subsystem initialized. PM1a_CNT: 0x{:X} (I/O: {}), DSDT S5: {:?}/{:?}",
        pm1a_cnt,
        is_io,
        slp_typa,
        slp_typb
    );
}

/// Helper to parse AML integers. Mutates pointer `p`.
fn parse_aml_integer(slice: &[u8], p: &mut usize) -> Option<u8> {
    if *p >= slice.len() {
        return None;
    }
    let opcode = slice[*p];
    *p += 1;
    match opcode {
        0x00 => Some(0),
        0x01 => Some(1),
        0x02..=0x08 => Some(opcode - 0x02 + 2),
        0x0A => {
            if *p < slice.len() {
                let val = slice[*p];
                *p += 1;
                Some(val)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Scan DSDT AML byte slice for _S5_ name and extract its SLP_TYPa and SLP_TYPb values.
fn scan_s5_values(aml: &[u8]) -> Option<(u8, u8)> {
    let mut s5_idx = None;
    // Scan for "_S5_"
    for i in 0..(aml.len().saturating_sub(4)) {
        if &aml[i..i + 4] == b"_S5_" {
            s5_idx = Some(i);
            break;
        }
    }

    let idx = s5_idx?;
    // Look for PackageOp (0x12) in the immediate vicinity
    let mut pkg_idx = None;
    let search_limit = (idx + 12).min(aml.len());
    for j in (idx + 4)..search_limit {
        if aml[j] == 0x12 {
            pkg_idx = Some(j);
            break;
        }
    }

    let p_idx = pkg_idx?;
    let mut p = p_idx + 1;

    // Skip Package Length (1 to 4 bytes in AML)
    if p < aml.len() {
        let lead = aml[p];
        let bytes_count = match lead >> 6 {
            0 => 1,
            1 => 2,
            2 => 3,
            3 => 4,
            _ => 1,
        };
        p += bytes_count;
    }

    // Skip number of elements (1 byte)
    p += 1;

    // Parse SLP_TYPa
    let typa = parse_aml_integer(aml, &mut p)?;
    // Parse SLP_TYPb
    let typb = parse_aml_integer(aml, &mut p).unwrap_or(0);

    Some((typa, typb))
}

/// Shutdown the system using ACPI PM1a/b registers.
/// Falls back to direct QEMU/Bochs/debug-exit writes if ACPI is not fully parsed.
pub fn shutdown() -> ! {
    let state = ACPI_STATE.lock();

    if let (Some(pm1a), Some(typa)) = (state.pm1a_cnt_blk, state.slp_typa) {
        // Bit 13 is SLP_EN
        let val_a = ((typa as u16) << 10) | (1 << 13);
        if state.is_io {
            let mut port_a = Port::<u16>::new(pm1a as u16);
            unsafe {
                port_a.write(val_a);
            }

            if let (Some(pm1b), Some(typb)) = (state.pm1b_cnt_blk, state.slp_typb) {
                let val_b = ((typb as u16) << 10) | (1 << 13);
                let mut port_b = Port::<u16>::new(pm1b as u16);
                unsafe {
                    port_b.write(val_b);
                }
            }
        } else if let Some(offset) = state.phys_mem_offset {
            // MMIO write
            let addr_a = (offset + pm1a as u64) as *mut u16;
            unsafe {
                core::ptr::write_volatile(addr_a, val_a);
            }
            if let (Some(pm1b), Some(typb)) = (state.pm1b_cnt_blk, state.slp_typb) {
                let val_b = ((typb as u16) << 10) | (1 << 13);
                let addr_b = (offset + pm1b as u64) as *mut u16;
                unsafe {
                    core::ptr::write_volatile(addr_b, val_b);
                }
            }
        }
    }

    // --- FALLBACKS ---

    // 1. Common QEMU/Bochs ACPI Poweroff Port: write 0x2000 (SLP_EN) or 0x3400 (SLP_TYP=5 | SLP_EN)
    // Try both standard PIIX4 (0xB004) and Q35 (0x0604) I/O PM1a_CNT ports
    for port_addr in &[0x604u16, 0xB004u16] {
        let mut port = Port::<u16>::new(*port_addr);
        unsafe {
            port.write(0x2000);   // SLP_EN (SLP_TYP=0)
            port.write(0x2000 | (5 << 10)); // SLP_EN | (SLP_TYP=5)
        }
    }

    // 2. QEMU ISA debug exit port (if configured with -device isa-debug-exit)
    let mut debug_port = Port::<u32>::new(0x501);
    unsafe {
        debug_port.write(0); // Exit status 0
    }

    // Loop forever if shutdown failed
    loop {
        x86_64::instructions::hlt();
    }
}

/// Reboot the system using ACPI Reset Register.
/// Returns false if not supported or write fails, allowing fallbacks.
pub fn reboot() -> bool {
    let state = ACPI_STATE.lock();
    if let (Some(reg), Some(val)) = (state.reset_reg, state.reset_value) {
        if reg.address_space == 1 {
            // System I/O port
            let mut port = Port::<u8>::new(reg.address.try_into().unwrap_or(0));
            unsafe {
                port.write(val);
            }
            return true;
        } else if reg.address_space == 0 {
            // System Memory MMIO
            if let Some(offset) = state.phys_mem_offset {
                let addr = (offset + reg.address) as *mut u8;
                unsafe {
                    core::ptr::write_volatile(addr, val);
                }
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_s5_values() {
        // Construct a mock DSDT containing AML bytecode for:
        // Name(_S5, Package(0x02) { 0x05, 0x05 })
        // Byte prefix 0x08 -> NameOp
        // 0x5F, 0x53, 0x35, 0x5F -> "_S5_"
        // 0x12 -> PackageOp
        // 0x06 -> Package length (small, so 1 byte)
        // 0x02 -> Element count
        // 0x0A, 0x05 -> First element: BytePrefix + value 5
        // 0x0A, 0x05 -> Second element: BytePrefix + value 5
        let aml = vec![
            0x08, 0x5F, 0x53, 0x35, 0x5F, 0x12, 0x06, 0x02, 0x0A, 0x05, 0x0A, 0x05,
        ];
        let res = scan_s5_values(&aml);
        assert_eq!(res, Some((5, 5)));

        // Test with ZeroOp (0x00) constants (standard for QEMU S5 package)
        let aml_zero = vec![
            0x08, 0x5F, 0x53, 0x35, 0x5F, 0x12, 0x04, 0x02, 0x00, 0x00,
        ];
        let res_zero = scan_s5_values(&aml_zero);
        assert_eq!(res_zero, Some((0, 0)));
    }
}
