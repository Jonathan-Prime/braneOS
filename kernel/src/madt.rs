//! ACPI MADT (Multiple APIC Description Table) parser.
//!
//! This module only decodes bytes; mapping the physical table and programming
//! the APIC controllers belongs to the SMP bring-up phase.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuEntry {
    pub processor_uid: u8,
    pub apic_id: u8,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoApicEntry {
    pub id: u8,
    pub address: u32,
    pub global_irq_base: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadtInfo {
    pub local_apic_address: u32,
    pub cpus: [Option<CpuEntry>; 32],
    pub cpu_count: usize,
    pub io_apics: [Option<IoApicEntry>; 8],
    pub io_apic_count: usize,
}

impl MadtInfo {
    /// Number of logical processors marked enabled by firmware.
    pub fn enabled_cpu_count(&self) -> usize {
        self.cpus[..self.cpu_count]
            .iter()
            .flatten()
            .filter(|cpu| cpu.enabled)
            .count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    TooShort,
    InvalidSignature,
    InvalidLength,
    InvalidChecksum,
}

pub fn parse(table: &[u8]) -> Result<MadtInfo, ParseError> {
    if table.len() < 44 {
        return Err(ParseError::TooShort);
    }
    if &table[..4] != b"APIC" {
        return Err(ParseError::InvalidSignature);
    }
    let length = u32::from_le_bytes(table[4..8].try_into().unwrap()) as usize;
    if length < 44 || length > table.len() {
        return Err(ParseError::InvalidLength);
    }
    if table[..length].iter().fold(0u8, |s, b| s.wrapping_add(*b)) != 0 {
        return Err(ParseError::InvalidChecksum);
    }
    let mut info = MadtInfo {
        local_apic_address: u32::from_le_bytes(table[36..40].try_into().unwrap()),
        cpus: [None; 32],
        cpu_count: 0,
        io_apics: [None; 8],
        io_apic_count: 0,
    };
    let mut offset = 44;
    while offset < length {
        if offset + 2 > length {
            return Err(ParseError::InvalidLength);
        }
        let kind = table[offset];
        let entry_len = table[offset + 1] as usize;
        if entry_len < 2 || offset + entry_len > length {
            return Err(ParseError::InvalidLength);
        }
        match kind {
            0 if entry_len >= 8 && info.cpu_count < info.cpus.len() => {
                info.cpus[info.cpu_count] = Some(CpuEntry {
                    processor_uid: table[offset + 2],
                    apic_id: table[offset + 3],
                    enabled: u32::from_le_bytes(table[offset + 4..offset + 8].try_into().unwrap())
                        & 1
                        != 0,
                });
                info.cpu_count += 1;
            }
            1 if entry_len >= 12 && info.io_apic_count < info.io_apics.len() => {
                info.io_apics[info.io_apic_count] = Some(IoApicEntry {
                    id: table[offset + 2],
                    address: u32::from_le_bytes(table[offset + 4..offset + 8].try_into().unwrap()),
                    global_irq_base: u32::from_le_bytes(
                        table[offset + 8..offset + 12].try_into().unwrap(),
                    ),
                });
                info.io_apic_count += 1;
            }
            _ => {}
        }
        offset += entry_len;
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;
    fn table(entries: &[u8]) -> Vec<u8> {
        let mut t = vec![0; 44];
        t[..4].copy_from_slice(b"APIC");
        t[36..40].copy_from_slice(&0xFEE0_0000u32.to_le_bytes());
        t.extend_from_slice(entries);
        let l = t.len() as u32;
        t[4..8].copy_from_slice(&l.to_le_bytes());
        let sum = t.iter().fold(0u8, |s, b| s.wrapping_add(*b));
        t[9] = 0u8.wrapping_sub(sum);
        t
    }
    #[test]
    fn parses_cpu_and_ioapic() {
        let t = table(&[
            0, 8, 2, 7, 1, 0, 0, 0, 1, 12, 3, 0, 0x00, 0x00, 0xE0, 0xFE, 0, 0, 0, 0,
        ]);
        let i = parse(&t).unwrap();
        assert_eq!(i.cpu_count, 1);
        assert_eq!(i.cpus[0].unwrap().apic_id, 7);
        assert_eq!(i.io_apic_count, 1);
        assert_eq!(i.io_apics[0].unwrap().address, 0xFEE0_0000);
    }
    #[test]
    fn rejects_truncated_entry() {
        let mut t = table(&[0, 8, 0]);
        let len = t.len() as u32;
        t[4..8].copy_from_slice(&len.to_le_bytes());
        assert_eq!(parse(&t), Err(ParseError::InvalidLength));
    }

    #[test]
    fn counts_only_enabled_cpus() {
        let t = table(&[0, 8, 0, 1, 1, 0, 0, 0, 0, 8, 1, 2, 0, 0, 0, 0]);
        assert_eq!(parse(&t).unwrap().enabled_cpu_count(), 1);
    }

    #[test]
    fn rejects_non_madt_signature() {
        let mut t = table(&[]);
        t[..4].copy_from_slice(b"FACP");
        assert_eq!(parse(&t), Err(ParseError::InvalidSignature));
    }
}
