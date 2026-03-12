#![allow(dead_code)]
// ============================================================
// Brane OS Kernel — Module Loader (Adaptability)
// ============================================================
//
// Manages loadable kernel sub-branes (modules).
// Supports loading, unloading, and querying module status.
//
// This is the foundation for runtime adaptability: modules
// can be hot-swapped without rebooting the system.
//
// Spec reference: ARCHITECTURE.md §5.2.8, §11
// ============================================================

use spin::Mutex;

// -----------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------

pub type ModuleId = u64;

/// Status of a loaded module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleStatus {
    Loaded,
    Running,
    Suspended,
    Unloading,
    Failed(i32),
}

/// Metadata about a loaded module.
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub id: ModuleId,
    pub name: [u8; 32],
    pub name_len: usize,
    pub version_major: u8,
    pub version_minor: u8,
    pub version_patch: u8,
    pub status: ModuleStatus,
    pub depends_on: [Option<ModuleId>; 4],
}

impl ModuleInfo {
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("???")
    }
}

// -----------------------------------------------------------------------
// Module Loader
// -----------------------------------------------------------------------

const MAX_MODULES: usize = 32;

pub struct ModuleLoader {
    modules: [Option<ModuleInfo>; MAX_MODULES],
    next_id: ModuleId,
}

impl ModuleLoader {
    const fn new() -> Self {
        const NONE: Option<ModuleInfo> = None;
        Self {
            modules: [NONE; MAX_MODULES],
            next_id: 1,
        }
    }

    /// Load a module by name and version.
    ///
    /// In a real implementation, `_image` would contain the module
    /// binary. For now, we just register the metadata.
    pub fn load(
        &mut self,
        name: &str,
        version: (u8, u8, u8),
        deps: &[ModuleId],
    ) -> Result<ModuleId, ModuleError> {
        // Check for duplicate names
        for info in self.modules.iter().flatten() {
            if info.name_str() == name {
                return Err(ModuleError::AlreadyLoaded);
            }
        }

        for slot in self.modules.iter_mut() {
            if slot.is_none() {
                let id = self.next_id;
                self.next_id += 1;

                let mut name_buf = [0u8; 32];
                let len = name.len().min(32);
                name_buf[..len].copy_from_slice(&name.as_bytes()[..len]);

                let mut depends_on = [None; 4];
                for (i, &dep) in deps.iter().take(4).enumerate() {
                    depends_on[i] = Some(dep);
                }

                *slot = Some(ModuleInfo {
                    id,
                    name: name_buf,
                    name_len: len,
                    version_major: version.0,
                    version_minor: version.1,
                    version_patch: version.2,
                    status: ModuleStatus::Loaded,
                    depends_on,
                });

                crate::serial_println!(
                    "[mod]  Loaded module '{}' v{}.{}.{} (id={})",
                    name,
                    version.0,
                    version.1,
                    version.2,
                    id
                );
                return Ok(id);
            }
        }
        Err(ModuleError::TableFull)
    }

    /// Start a loaded module (transition to Running).
    pub fn start(&mut self, id: ModuleId) -> Result<(), ModuleError> {
        self.set_status(id, ModuleStatus::Running)
    }

    /// Suspend a running module.
    pub fn suspend(&mut self, id: ModuleId) -> Result<(), ModuleError> {
        self.set_status(id, ModuleStatus::Suspended)
    }

    /// Unload a module by ID.
    pub fn unload(&mut self, id: ModuleId) -> Result<(), ModuleError> {
        // Check no other module depends on this one
        for info in self.modules.iter().flatten() {
            if info.id != id {
                for dep in &info.depends_on {
                    if *dep == Some(id) {
                        return Err(ModuleError::HasDependents);
                    }
                }
            }
        }

        for slot in self.modules.iter_mut() {
            if let Some(info) = slot {
                if info.id == id {
                    crate::serial_println!("[mod]  Unloaded module '{}'", info.name_str());
                    *slot = None;
                    return Ok(());
                }
            }
        }
        Err(ModuleError::NotFound)
    }

    /// Get information about a module.
    pub fn info(&self, id: ModuleId) -> Option<&ModuleInfo> {
        self.modules
            .iter()
            .filter_map(|s| s.as_ref())
            .find(|m| m.id == id)
    }

    /// List all loaded modules.
    pub fn list(&self) -> impl Iterator<Item = &ModuleInfo> {
        self.modules.iter().filter_map(|s| s.as_ref())
    }

    /// Number of loaded modules.
    pub fn loaded_count(&self) -> usize {
        self.modules.iter().filter(|s| s.is_some()).count()
    }

    fn set_status(&mut self, id: ModuleId, status: ModuleStatus) -> Result<(), ModuleError> {
        for info in self.modules.iter_mut().flatten() {
            if info.id == id {
                info.status = status;
                return Ok(());
            }
        }
        Err(ModuleError::NotFound)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleError {
    NotFound,
    AlreadyLoaded,
    TableFull,
    HasDependents,
    InvalidImage,
}

/// Global module loader.
pub static MODULE_LOADER: Mutex<ModuleLoader> = Mutex::new(ModuleLoader::new());
