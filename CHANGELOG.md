# Changelog

All notable changes to Brane OS are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- Portable UEFI ISO packaging with BIOS and UEFI disk artifacts.
- Automated ISO boot verification with QEMU, OVMF and TCG.
- Tag-driven GitHub release workflow with SHA-256 verification.

## 0.1.0 — Foundation

### Added

- Bootable x86_64 kernel with GDT, IDT, PIC, paging and heap allocation.
- Cooperative scheduler, syscalls, IPC, processes and POSIX-style signals.
- Capability security model, audit log and restricted AI observer.
- VFS, RamFS, TTY, `brsh`, networking and Brane Protocol v2.
- ACPI shutdown, reboot and tested S3 suspend/resume.
- Unit, stress, mutation-fuzz, security, integration and E2E suites.

### Known limitations

- Single-core execution; SMP/APIC support is planned for Phase 12.
- USB xHCI and persistent block storage are planned for Phase 13.
- FAT32 support currently provides structural parsing rather than full reads.
