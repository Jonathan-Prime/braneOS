//! SMP topology and Application Processor (AP) boot state.
//!
//! This module deliberately stops at a validated boot plan. Starting APs
//! requires a low-memory trampoline, per-CPU stacks and page-table hand-off;
//! those hardware writes are kept out of discovery so a malformed MADT can
//! never leave half-started processors behind.

use crate::madt::MadtInfo;

pub const MAX_CPUS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CpuLifecycle {
    Disabled = 0,
    Discovered = 1,
    Starting = 2,
    Online = 3,
    Failed = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuDescriptor {
    pub processor_uid: u32,
    pub apic_id: u32,
    pub enabled: bool,
    pub lifecycle: CpuLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuBootPlan {
    pub cpus: [Option<CpuDescriptor>; MAX_CPUS],
    pub cpu_count: usize,
    pub enabled_cpu_count: usize,
    pub bsp_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuPlanError {
    NoEnabledCpu,
    DuplicateApicId(u32),
    BspAlreadyAssigned,
    BspNotFound(u32),
    InvalidState,
}

impl CpuBootPlan {
    /// Build a deterministic plan from MADT entries without touching APIC
    /// registers or allocating per-CPU memory.
    pub fn from_madt(info: &MadtInfo) -> Result<Self, CpuPlanError> {
        let mut plan = Self {
            cpus: [None; MAX_CPUS],
            cpu_count: 0,
            enabled_cpu_count: 0,
            bsp_index: None,
        };
        for entry in info.cpus.iter().take(info.cpu_count).flatten() {
            if plan.cpu_count == MAX_CPUS {
                break;
            }
            if plan
                .cpus
                .iter()
                .take(plan.cpu_count)
                .flatten()
                .any(|cpu| cpu.apic_id == entry.apic_id)
            {
                return Err(CpuPlanError::DuplicateApicId(entry.apic_id));
            }
            let lifecycle = if entry.enabled {
                plan.enabled_cpu_count += 1;
                CpuLifecycle::Discovered
            } else {
                CpuLifecycle::Disabled
            };
            plan.cpus[plan.cpu_count] = Some(CpuDescriptor {
                processor_uid: entry.processor_uid,
                apic_id: entry.apic_id,
                enabled: entry.enabled,
                lifecycle,
            });
            plan.cpu_count += 1;
        }
        if plan.enabled_cpu_count == 0 {
            return Err(CpuPlanError::NoEnabledCpu);
        }
        Ok(plan)
    }

    /// Mark the processor whose Local APIC ID was read during bring-up as the
    /// bootstrap processor. Exactly one BSP is allowed in a boot plan.
    pub fn assign_bsp(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        if self.bsp_index.is_some() {
            return Err(CpuPlanError::BspAlreadyAssigned);
        }
        let Some(index) = self
            .cpus
            .iter()
            .take(self.cpu_count)
            .position(|cpu| cpu.is_some_and(|cpu| cpu.enabled && cpu.apic_id == apic_id))
        else {
            return Err(CpuPlanError::BspNotFound(apic_id));
        };
        self.cpus[index].as_mut().unwrap().lifecycle = CpuLifecycle::Online;
        self.bsp_index = Some(index);
        Ok(index)
    }

    pub fn mark_starting(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        let index = self.find_enabled(apic_id)?;
        let cpu = self.cpus[index].as_mut().unwrap();
        if cpu.lifecycle != CpuLifecycle::Discovered {
            return Err(CpuPlanError::InvalidState);
        }
        cpu.lifecycle = CpuLifecycle::Starting;
        Ok(index)
    }

    pub fn mark_online(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        let index = self.find_enabled(apic_id)?;
        let cpu = self.cpus[index].as_mut().unwrap();
        if cpu.lifecycle != CpuLifecycle::Starting {
            return Err(CpuPlanError::InvalidState);
        }
        cpu.lifecycle = CpuLifecycle::Online;
        Ok(index)
    }

    pub fn mark_failed(&mut self, apic_id: u32) -> Result<usize, CpuPlanError> {
        let index = self.find_enabled(apic_id)?;
        let cpu = self.cpus[index].as_mut().unwrap();
        if cpu.lifecycle != CpuLifecycle::Starting {
            return Err(CpuPlanError::InvalidState);
        }
        cpu.lifecycle = CpuLifecycle::Failed;
        Ok(index)
    }

    pub fn online_cpu_count(&self) -> usize {
        self.cpus
            .iter()
            .take(self.cpu_count)
            .flatten()
            .filter(|cpu| cpu.lifecycle == CpuLifecycle::Online)
            .count()
    }

    fn find_enabled(&self, apic_id: u32) -> Result<usize, CpuPlanError> {
        self.cpus
            .iter()
            .take(self.cpu_count)
            .position(|cpu| cpu.is_some_and(|cpu| cpu.enabled && cpu.apic_id == apic_id))
            .ok_or(CpuPlanError::BspNotFound(apic_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn madt(cpus: &[(u32, u32, bool)]) -> MadtInfo {
        let mut info = MadtInfo {
            local_apic_address: 0xFEE0_0000,
            cpus: [None; MAX_CPUS],
            cpu_count: 0,
            io_apics: [None; 8],
            io_apic_count: 0,
            interrupt_overrides: [None; 16],
            interrupt_override_count: 0,
        };
        for &(uid, apic_id, enabled) in cpus {
            info.cpus[info.cpu_count] = Some(crate::madt::CpuEntry {
                processor_uid: uid,
                apic_id,
                enabled,
            });
            info.cpu_count += 1;
        }
        info
    }

    #[test]
    fn builds_plan_and_assigns_bsp() {
        let mut plan =
            CpuBootPlan::from_madt(&madt(&[(0, 2, true), (1, 7, true), (2, 9, false)])).unwrap();
        assert_eq!(plan.enabled_cpu_count, 2);
        assert_eq!(plan.assign_bsp(7), Ok(1));
        assert_eq!(plan.online_cpu_count(), 1);
        assert_eq!(plan.cpus[2].unwrap().lifecycle, CpuLifecycle::Disabled);
    }

    #[test]
    fn rejects_duplicate_apic_ids() {
        assert_eq!(
            CpuBootPlan::from_madt(&madt(&[(0, 2, true), (1, 2, true)])),
            Err(CpuPlanError::DuplicateApicId(2))
        );
    }

    #[test]
    fn rejects_empty_enabled_set_and_unknown_bsp() {
        assert_eq!(
            CpuBootPlan::from_madt(&madt(&[(0, 2, false)])),
            Err(CpuPlanError::NoEnabledCpu)
        );
        let mut plan = CpuBootPlan::from_madt(&madt(&[(0, 2, true)])).unwrap();
        assert_eq!(plan.assign_bsp(3), Err(CpuPlanError::BspNotFound(3)));
    }

    #[test]
    fn enforces_ap_lifecycle_transitions() {
        let mut plan = CpuBootPlan::from_madt(&madt(&[(0, 2, true), (1, 7, true)])).unwrap();
        assert_eq!(plan.mark_online(7), Err(CpuPlanError::InvalidState));
        assert_eq!(plan.mark_starting(7), Ok(1));
        assert_eq!(plan.mark_online(7), Ok(1));
        assert_eq!(plan.mark_failed(7), Err(CpuPlanError::InvalidState));
        assert_eq!(plan.assign_bsp(2), Ok(0));
        assert_eq!(plan.assign_bsp(7), Err(CpuPlanError::BspAlreadyAssigned));
    }
}
