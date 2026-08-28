//! ACPI MADT (Multiple APIC Description Table) parser.
//!
//! This module only decodes bytes; mapping the physical table and programming
//! the APIC controllers belongs to the SMP bring-up phase.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuEntry {
    pub processor_uid: u32,
    pub apic_id: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoApicEntry {
    pub id: u8,
    pub address: u32,
    pub global_irq_base: u32,
}

/// Legacy ISA interrupt remapping described by MADT entry type 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptSourceOverride {
    pub bus: u8,
    pub source_irq: u8,
    pub global_irq: u32,
    pub flags: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsaIrqRoute {
    pub global_irq: u32,
    pub active_low: bool,
    pub level_triggered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqRouteError {
    ReservedPolarity,
    ReservedTriggerMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadtInfo {
    pub local_apic_address: u64,
    pub cpus: [Option<CpuEntry>; 32],
    pub cpu_count: usize,
    pub io_apics: [Option<IoApicEntry>; 8],
    pub io_apic_count: usize,
    pub interrupt_overrides: [Option<InterruptSourceOverride>; 16],
    pub interrupt_override_count: usize,
}

impl MadtInfo {
    /// Number of logical processors marked enabled by firmware.
    pub fn enabled_cpu_count(&self) -> usize {
        self.cpus
            .iter()
            .take(self.cpu_count)
            .flatten()
            .filter(|cpu| cpu.enabled)
            .count()
    }

    /// Resolve an ISA IRQ to its Global System Interrupt and electrical mode.
    /// ISA defaults to active-high, edge-triggered when no override exists.
    pub fn isa_irq_route(&self, source_irq: u8) -> Result<IsaIrqRoute, IrqRouteError> {
        let Some(entry) = self
            .interrupt_overrides
            .iter()
            .take(self.interrupt_override_count)
            .flatten()
            .find(|entry| entry.bus == 0 && entry.source_irq == source_irq)
        else {
            return Ok(IsaIrqRoute {
                global_irq: source_irq as u32,
                active_low: false,
                level_triggered: false,
            });
        };

        let active_low = match entry.flags & 0b11 {
            // "Conforms" uses the ISA bus default.
            0 | 1 => false,
            3 => true,
            _ => return Err(IrqRouteError::ReservedPolarity),
        };
        let level_triggered = match (entry.flags >> 2) & 0b11 {
            // "Conforms" uses the ISA bus default.
            0 | 1 => false,
            3 => true,
            _ => return Err(IrqRouteError::ReservedTriggerMode),
        };
        Ok(IsaIrqRoute {
            global_irq: entry.global_irq,
            active_low,
            level_triggered,
        })
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
        local_apic_address: u32::from_le_bytes(table[36..40].try_into().unwrap()) as u64,
        cpus: [None; 32],
        cpu_count: 0,
        io_apics: [None; 8],
        io_apic_count: 0,
        interrupt_overrides: [None; 16],
        interrupt_override_count: 0,
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
            // Processor Local APIC (ACPI 6.5, MADT type 0).
            0 => {
                if entry_len < 8 {
                    return Err(ParseError::InvalidLength);
                }
                if info.cpu_count < info.cpus.len() {
                    info.cpus[info.cpu_count] = Some(CpuEntry {
                        processor_uid: table[offset + 2] as u32,
                        apic_id: table[offset + 3] as u32,
                        enabled: u32::from_le_bytes(
                            table[offset + 4..offset + 8].try_into().unwrap(),
                        ) & 1
                            != 0,
                    });
                    info.cpu_count += 1;
                }
            }
            // I/O APIC (ACPI 6.5, MADT type 1).
            1 => {
                if entry_len < 12 {
                    return Err(ParseError::InvalidLength);
                }
                if info.io_apic_count < info.io_apics.len() {
                    info.io_apics[info.io_apic_count] = Some(IoApicEntry {
                        id: table[offset + 2],
                        address: u32::from_le_bytes(
                            table[offset + 4..offset + 8].try_into().unwrap(),
                        ),
                        global_irq_base: u32::from_le_bytes(
                            table[offset + 8..offset + 12].try_into().unwrap(),
                        ),
                    });
                    info.io_apic_count += 1;
                }
            }
            // Interrupt Source Override (ACPI 6.5, MADT type 2).
            2 => {
                if entry_len < 10 {
                    return Err(ParseError::InvalidLength);
                }
                if info.interrupt_override_count < info.interrupt_overrides.len() {
                    info.interrupt_overrides[info.interrupt_override_count] =
                        Some(InterruptSourceOverride {
                            bus: table[offset + 2],
                            source_irq: table[offset + 3],
                            global_irq: u32::from_le_bytes(
                                table[offset + 4..offset + 8].try_into().unwrap(),
                            ),
                            flags: u16::from_le_bytes(
                                table[offset + 8..offset + 10].try_into().unwrap(),
                            ),
                        });
                    info.interrupt_override_count += 1;
                }
            }
            // Local APIC Address Override (ACPI 6.5, MADT type 5).
            5 => {
                if entry_len < 12 {
                    return Err(ParseError::InvalidLength);
                }
                info.local_apic_address =
                    u64::from_le_bytes(table[offset + 4..offset + 12].try_into().unwrap());
            }
            // Processor Local x2APIC (ACPI 6.5, MADT type 9).
            9 => {
                if entry_len < 16 {
                    return Err(ParseError::InvalidLength);
                }
                if info.cpu_count < info.cpus.len() {
                    info.cpus[info.cpu_count] = Some(CpuEntry {
                        apic_id: u32::from_le_bytes(
                            table[offset + 4..offset + 8].try_into().unwrap(),
                        ),
                        processor_uid: u32::from_le_bytes(
                            table[offset + 12..offset + 16].try_into().unwrap(),
                        ),
                        enabled: u32::from_le_bytes(
                            table[offset + 8..offset + 12].try_into().unwrap(),
                        ) & 1
                            != 0,
                    });
                    info.cpu_count += 1;
                }
            }
            // Unknown entries are intentionally skipped for forward
            // compatibility; their length was validated above.
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

    #[test]
    fn parses_lapic_address_override_and_x2apic_cpu() {
        let mut entries = vec![5, 12, 0, 0];
        entries.extend_from_slice(&0x1_0000_0000u64.to_le_bytes());
        entries.extend_from_slice(&[
            9, 16, 0, 0, // type, length, reserved
            0x34, 0x12, 0, 0, // x2APIC ID
            1, 0, 0, 0, // enabled
            7, 0, 0, 0, // processor UID
        ]);
        let info = parse(&table(&entries)).unwrap();
        assert_eq!(info.local_apic_address, 0x1_0000_0000);
        assert_eq!(info.cpu_count, 1);
        assert_eq!(info.cpus[0].unwrap().processor_uid, 7);
        assert_eq!(info.cpus[0].unwrap().apic_id, 0x1234);
        assert_eq!(info.enabled_cpu_count(), 1);
    }

    #[test]
    fn rejects_short_known_entry() {
        assert_eq!(parse(&table(&[1, 2])), Err(ParseError::InvalidLength));
        assert_eq!(parse(&table(&[2, 2])), Err(ParseError::InvalidLength));
        assert_eq!(parse(&table(&[9, 2])), Err(ParseError::InvalidLength));
    }

    #[test]
    fn resolves_isa_interrupt_source_override() {
        let info = parse(&table(&[
            2, 10, 0, 0, // type, length, ISA bus, IRQ 0
            2, 0, 0, 0, // GSI 2
            0x0f, 0, // active-low, level-triggered
        ]))
        .unwrap();
        assert_eq!(info.interrupt_override_count, 1);
        assert_eq!(
            info.isa_irq_route(0),
            Ok(IsaIrqRoute {
                global_irq: 2,
                active_low: true,
                level_triggered: true,
            })
        );
        assert_eq!(
            info.isa_irq_route(1),
            Ok(IsaIrqRoute {
                global_irq: 1,
                active_low: false,
                level_triggered: false,
            })
        );
    }

    #[test]
    fn rejects_reserved_override_modes_when_routing() {
        let info = parse(&table(&[2, 10, 0, 1, 1, 0, 0, 0, 2, 0])).unwrap();
        assert_eq!(info.isa_irq_route(1), Err(IrqRouteError::ReservedPolarity));

        let info = parse(&table(&[2, 10, 0, 1, 1, 0, 0, 0, 8, 0])).unwrap();
        assert_eq!(
            info.isa_irq_route(1),
            Err(IrqRouteError::ReservedTriggerMode)
        );
    }
}
