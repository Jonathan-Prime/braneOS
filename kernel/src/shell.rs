// ============================================================
// Brane OS Kernel — Interactive Shell (brsh)
// ============================================================
//
// A minimal in-kernel shell for system interaction.
// Runs in kernel mode (no user-space yet).
//
// Commands:
//   help, ps, mem, lsmod, brane status, ai status,
//   caps, audit, ls, cat, clear, reboot
//
// Spec reference: ARCHITECTURE.md §5.3 (planned)
// ============================================================

use crate::{
    ai, audit, brane, dns, memory, module_loader, net, process, sched, security, serial_println,
    socket, tty, vfs,
};

/// Print the shell prompt.
pub fn prompt() {
    tty::tty_print("brane> ");
}

/// Execute a single shell command line.
pub fn execute(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }

    // Split into command and arguments
    let (cmd, args) = match line.find(' ') {
        Some(pos) => (&line[..pos], line[pos + 1..].trim()),
        None => (line, ""),
    };

    match cmd {
        "help" => cmd_help(),
        "ps" => cmd_ps(),
        "mem" => cmd_mem(),
        "lsmod" => cmd_lsmod(),
        "brane" => cmd_brane(args),
        "ai" => cmd_ai(args),
        "caps" => cmd_caps(),
        "audit" => cmd_audit(),
        "ls" => cmd_ls(args),
        "cat" => cmd_cat(args),
        "net" => cmd_net(args),
        "dns" => cmd_dns(args),
        "sockets" => cmd_sockets(),
        "clear" => cmd_clear(),
        "reboot" => cmd_reboot(),
        "sched" => cmd_sched(),
        _ => {
            tty::tty_print("Unknown command: ");
            tty::tty_println(cmd);
            tty::tty_println("Type 'help' for available commands.");
        }
    }
}

// -----------------------------------------------------------------------
// Built-in Commands
// -----------------------------------------------------------------------

fn cmd_help() {
    tty::tty_println("Brane OS Shell (brsh) — Available commands:");
    tty::tty_println("");
    tty::tty_println("  help          Show this help");
    tty::tty_println("  ps            List processes");
    tty::tty_println("  mem           Memory statistics");
    tty::tty_println("  sched         Scheduler status");
    tty::tty_println("  lsmod         List loaded modules");
    tty::tty_println("  brane status  Brane protocol status");
    tty::tty_println("  ai status     AI engine status");
    tty::tty_println("  caps          List capabilities");
    tty::tty_println("  audit         Recent audit entries");
    tty::tty_println("  ls [path]     List directory");
    tty::tty_println("  cat <path>    Read file contents");
    tty::tty_println("  net status    Network interface info");
    tty::tty_println("  dns <host>    Resolve hostname");
    tty::tty_println("  sockets       List open sockets");
    tty::tty_println("  clear         Clear screen");
    tty::tty_println("  reboot        Reboot system");
}

fn cmd_ps() {
    tty::tty_println("PID  NAME                  STATE");
    tty::tty_println("---  --------------------  ---------");
    let table = process::PROCESS_TABLE.lock();
    for proc in table.active_processes() {
        use core::fmt::Write;
        let mut buf = [0u8; 128];
        let mut cursor = WriteBuf::new(&mut buf);
        let _ = write!(
            cursor,
            "{:<4} {:<22} {:?}",
            proc.pid,
            proc.name_str(),
            proc.state
        );
        tty::tty_println(cursor.as_str());
    }
}

fn cmd_mem() {
    use core::fmt::Write;
    let mut buf = [0u8; 256];
    let mut c = WriteBuf::new(&mut buf);

    let free = memory::frame_allocator::free_frame_count();
    let total_kb = (free * 4096) / 1024;
    let _ = writeln!(c, "Physical Memory:");
    let _ = writeln!(c, "  Free frames:  {} ({} KiB)", free, total_kb);
    let _ = writeln!(c);
    let _ = writeln!(c, "Kernel Heap:");
    let _ = writeln!(
        c,
        "  Region: 0x{:X} .. 0x{:X} ({} KiB)",
        memory::heap::HEAP_START,
        memory::heap::HEAP_START + memory::heap::HEAP_SIZE,
        memory::heap::HEAP_SIZE / 1024
    );
    tty::tty_print(c.as_str());
}

fn cmd_sched() {
    use core::fmt::Write;
    let mut buf = [0u8; 128];
    let mut c = WriteBuf::new(&mut buf);
    let scheduler = sched::SCHEDULER.lock();
    let _ = writeln!(c, "Scheduler: {} active tasks", scheduler.active_count());
    tty::tty_print(c.as_str());
}

fn cmd_lsmod() {
    tty::tty_println("ID  NAME                  VERSION  STATE");
    tty::tty_println("--  --------------------  -------  -----");
    let loader = module_loader::MODULE_LOADER.lock();
    for info in loader.list() {
        use core::fmt::Write;
        let mut buf = [0u8; 128];
        let mut c = WriteBuf::new(&mut buf);
        let _ = write!(
            c,
            "{:<3} {:<22} {}.{}.{}    {:?}",
            info.id,
            info.name_str(),
            info.version_major,
            info.version_minor,
            info.version_patch,
            info.status
        );
        tty::tty_println(c.as_str());
    }
}

fn cmd_brane(args: &str) {
    if args != "status" && !args.is_empty() {
        tty::tty_println("Usage: brane status");
        return;
    }

    use core::fmt::Write;
    let brane_mgr = brane::BRANE.lock();
    let mut buf = [0u8; 256];
    let mut c = WriteBuf::new(&mut buf);
    let _ = writeln!(c, "Brane Protocol Status:");
    let _ = writeln!(c, "  Discovered:     {}", brane_mgr.discovered_count());
    let _ = writeln!(c, "  Active sessions: {}", brane_mgr.active_session_count());
    tty::tty_print(c.as_str());
}

fn cmd_ai(args: &str) {
    if args != "status" && !args.is_empty() {
        tty::tty_println("Usage: ai status");
        return;
    }

    use core::fmt::Write;
    let engine = ai::AI_ENGINE.lock();
    let stats = engine.stats();
    let mut buf = [0u8; 256];
    let mut c = WriteBuf::new(&mut buf);
    let _ = writeln!(c, "AI Engine Status:");
    let _ = writeln!(c, "  Mode:           {:?}", stats.mode);
    let _ = writeln!(c, "  Observations:   {}", stats.total_observations);
    let _ = writeln!(c, "  Actions taken:  {}", stats.total_actions_executed);
    tty::tty_print(c.as_str());
}

fn cmd_caps() {
    use core::fmt::Write;
    let cap_mgr = security::CAP_MANAGER.lock();
    let mut buf = [0u8; 128];
    let mut c = WriteBuf::new(&mut buf);
    let _ = writeln!(c, "Active capabilities: {}", cap_mgr.active_count());
    tty::tty_print(c.as_str());
}

fn cmd_audit() {
    use core::fmt::Write;
    let audit_log = audit::AUDIT.lock();
    let mut buf = [0u8; 128];
    let mut c = WriteBuf::new(&mut buf);
    let _ = writeln!(c, "Audit log: {} total events", audit_log.total_events());
    tty::tty_print(c.as_str());
}

fn cmd_ls(args: &str) {
    let path = if args.is_empty() { "/" } else { args };

    let vfs_mgr = vfs::VFS.lock();
    let mut entries = [vfs::DirEntry {
        name: [0; vfs::MAX_NAME],
        name_len: 0,
        node_type: vfs::NodeType::File,
    }; 32];

    match vfs_mgr.readdir(path, &mut entries) {
        Ok(count) => {
            for entry in &entries[..count] {
                let type_char = match entry.node_type {
                    vfs::NodeType::Directory => "d",
                    vfs::NodeType::File => "-",
                    vfs::NodeType::Device => "c",
                };
                use core::fmt::Write;
                let mut buf = [0u8; 128];
                let mut c = WriteBuf::new(&mut buf);
                let _ = write!(c, "{}  {}", type_char, entry.name_str());
                tty::tty_println(c.as_str());
            }
            if count == 0 {
                tty::tty_println("(empty)");
            }
        }
        Err(e) => {
            use core::fmt::Write;
            let mut buf = [0u8; 128];
            let mut c = WriteBuf::new(&mut buf);
            let _ = write!(c, "ls: error: {:?}", e);
            tty::tty_println(c.as_str());
        }
    }
}

fn cmd_cat(args: &str) {
    if args.is_empty() {
        tty::tty_println("Usage: cat <path>");
        return;
    }

    let vfs_mgr = vfs::VFS.lock();
    let mut buf = [0u8; 4096];
    match vfs_mgr.read(args, 0, &mut buf) {
        Ok(n) => {
            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                tty::tty_print(s);
                if !s.ends_with('\n') {
                    tty::tty_println("");
                }
            } else {
                use core::fmt::Write;
                let mut out = [0u8; 64];
                let mut c = WriteBuf::new(&mut out);
                let _ = write!(c, "(binary data, {} bytes)", n);
                tty::tty_println(c.as_str());
            }
        }
        Err(e) => {
            use core::fmt::Write;
            let mut out = [0u8; 128];
            let mut c = WriteBuf::new(&mut out);
            let _ = write!(c, "cat: error: {:?}", e);
            tty::tty_println(c.as_str());
        }
    }
}

fn cmd_net(args: &str) {
    if args != "status" && !args.is_empty() {
        tty::tty_println("Usage: net status");
        return;
    }

    let stack = net::NET_STACK.lock();
    if !stack.initialized {
        tty::tty_println("Network: not initialized (no virtio-net device found)");
        return;
    }

    use core::fmt::Write;
    let mut buf = [0u8; 512];
    let mut c = WriteBuf::new(&mut buf);
    let _ = writeln!(c, "Network Interface: eth0 (virtio-net)");
    let _ = writeln!(
        c,
        "  MAC:       {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        stack.info.mac[0],
        stack.info.mac[1],
        stack.info.mac[2],
        stack.info.mac[3],
        stack.info.mac[4],
        stack.info.mac[5]
    );
    let _ = writeln!(
        c,
        "  IPv4:      {}.{}.{}.{}/{}",
        stack.info.ip[0], stack.info.ip[1], stack.info.ip[2], stack.info.ip[3], stack.info.prefix
    );
    let _ = writeln!(
        c,
        "  Gateway:   {}.{}.{}.{}",
        stack.info.gateway[0], stack.info.gateway[1], stack.info.gateway[2], stack.info.gateway[3]
    );
    let _ = writeln!(
        c,
        "  Link:      {}",
        if stack.info.link_up { "UP" } else { "DOWN" }
    );
    let _ = writeln!(c, "  TX:        {} packets", stack.info.packets_tx);
    let _ = writeln!(c, "  RX:        {} packets", stack.info.packets_rx);
    tty::tty_print(c.as_str());
}

fn cmd_dns(args: &str) {
    if args.is_empty() {
        // List all DNS entries
        let resolver = dns::DNS.lock();
        use core::fmt::Write;
        let mut buf = [0u8; 512];
        let mut c = WriteBuf::new(&mut buf);
        let _ = writeln!(c, "DNS Host Table ({} entries):", resolver.host_count());
        for (name, addr) in resolver.list_hosts() {
            let _ = writeln!(
                c,
                "  {:<20} {}.{}.{}.{}",
                name, addr[0], addr[1], addr[2], addr[3]
            );
        }
        tty::tty_print(c.as_str());
        return;
    }

    let resolver = dns::DNS.lock();
    match resolver.resolve(args) {
        Some(addr) => {
            use core::fmt::Write;
            let mut buf = [0u8; 128];
            let mut c = WriteBuf::new(&mut buf);
            let _ = write!(
                c,
                "{} => {}.{}.{}.{}",
                args, addr[0], addr[1], addr[2], addr[3]
            );
            tty::tty_println(c.as_str());
        }
        None => {
            tty::tty_print("dns: host not found: ");
            tty::tty_println(args);
        }
    }
}

fn cmd_sockets() {
    let table = socket::SOCKET_TABLE.lock();
    let count = table.active_count();
    if count == 0 {
        tty::tty_println("No active sockets.");
        return;
    }
    tty::tty_println("ID   PROTO  STATE        LOCAL               REMOTE");
    tty::tty_println("---  -----  -----------  ------------------  ------------------");
    for sock in table.active_sockets() {
        use core::fmt::Write;
        let mut buf = [0u8; 128];
        let mut c = WriteBuf::new(&mut buf);
        let proto = match sock.protocol {
            socket::Protocol::Tcp => "TCP",
            socket::Protocol::Udp => "UDP",
        };
        let _ = write!(c, "{:<4} {:<6} {:?}", sock.id, proto, sock.state);
        tty::tty_println(c.as_str());
    }
}

fn cmd_clear() {
    // Clear serial (ANSI escape)
    serial_println!("\x1b[2J\x1b[H");
    // Clear framebuffer
    crate::framebuffer::fb_clear();
}

fn cmd_reboot() {
    tty::tty_println("Rebooting...");
    // Triple fault to reboot: load a zero-length IDT and trigger an interrupt
    unsafe {
        // Load invalid IDT
        let idt = x86_64::structures::idt::InterruptDescriptorTable::new();
        let ptr = x86_64::instructions::tables::DescriptorTablePointer {
            limit: 0,
            base: x86_64::VirtAddr::new(&idt as *const _ as u64),
        };
        x86_64::instructions::tables::lidt(&ptr);
        // Trigger interrupt → triple fault → reboot
        core::arch::asm!("int3");
    }
}

// -----------------------------------------------------------------------
// Utility: stack-allocated write buffer for formatting
// -----------------------------------------------------------------------

struct WriteBuf<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> WriteBuf<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.pos]).unwrap_or("")
    }
}

impl<'a> core::fmt::Write for WriteBuf<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len() - self.pos;
        let to_write = bytes.len().min(remaining);
        self.buf[self.pos..self.pos + to_write].copy_from_slice(&bytes[..to_write]);
        self.pos += to_write;
        Ok(())
    }
}
