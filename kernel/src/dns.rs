// ============================================================
// Brane OS Kernel — DNS Resolver
// ============================================================
//
// Minimal DNS stub with a static host table.
// Future: UDP-based DNS queries via the socket API.
//
// Reference: ARCHITECTURE.md §5.4 (planned)
// ============================================================

#![allow(dead_code)]

use spin::Mutex;

// -----------------------------------------------------------------------
// Host Table
// -----------------------------------------------------------------------

const MAX_HOSTS: usize = 16;
const MAX_HOSTNAME: usize = 64;

/// A static DNS host entry.
#[derive(Clone, Copy)]
struct HostEntry {
    name: [u8; MAX_HOSTNAME],
    name_len: usize,
    addr: [u8; 4],
    used: bool,
}

impl HostEntry {
    const fn empty() -> Self {
        Self {
            name: [0; MAX_HOSTNAME],
            name_len: 0,
            addr: [0; 4],
            used: false,
        }
    }

    fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

/// DNS resolver with static host table.
pub struct DnsResolver {
    hosts: [HostEntry; MAX_HOSTS],
    count: usize,
}

impl DnsResolver {
    const fn new() -> Self {
        Self {
            hosts: [HostEntry::empty(); MAX_HOSTS],
            count: 0,
        }
    }

    /// Add a static host entry.
    pub fn add_host(&mut self, name: &str, addr: [u8; 4]) -> bool {
        if self.count >= MAX_HOSTS {
            return false;
        }
        for slot in self.hosts.iter_mut() {
            if !slot.used {
                let len = name.len().min(MAX_HOSTNAME);
                slot.name[..len].copy_from_slice(&name.as_bytes()[..len]);
                slot.name_len = len;
                slot.addr = addr;
                slot.used = true;
                self.count += 1;
                return true;
            }
        }
        false
    }

    /// Resolve a hostname to an IPv4 address.
    pub fn resolve(&self, name: &str) -> Option<[u8; 4]> {
        for entry in &self.hosts {
            if entry.used && entry.name_str() == name {
                return Some(entry.addr);
            }
        }
        None
    }

    /// Number of registered hosts.
    pub fn host_count(&self) -> usize {
        self.count
    }

    /// List all host entries.
    pub fn list_hosts(&self) -> impl Iterator<Item = (&str, [u8; 4])> {
        self.hosts
            .iter()
            .filter(|h| h.used)
            .map(|h| (h.name_str(), h.addr))
    }
}

/// Global DNS resolver.
pub static DNS: Mutex<DnsResolver> = Mutex::new(DnsResolver::new());

/// Initialize DNS with standard entries.
pub fn init() {
    let mut dns = DNS.lock();
    dns.add_host("localhost", [127, 0, 0, 1]);
    dns.add_host("gateway", [10, 0, 2, 2]);
    dns.add_host("dns-server", [10, 0, 2, 3]);
    dns.add_host("brane-local", [10, 0, 2, 15]);
}
