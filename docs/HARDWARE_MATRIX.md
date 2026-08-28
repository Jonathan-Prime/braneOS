# Hardware validation matrix

This matrix records tests that must be run on physical machines before the
v1.0 gate. QEMU results are automated; physical rows require an operator to
fill in the firmware, hardware and observed result.

| Platform | Firmware | Boot | Keyboard | Network | ACPI S3 | Result | Date / notes |
|----------|----------|------|----------|---------|---------|--------|--------------|
| QEMU x86_64 (TCG) | BIOS | ✅ | ✅ | ✅/simulated | ✅ | Automated | `make test-all` |
| QEMU x86_64 (TCG) | UEFI/OVMF | ✅ | ✅ | ✅/simulated | ✅ | Automated | `make iso-test VERSION=dev` |
| Mac Intel reference machine | BIOS | ☐ | ☐ | ☐ | ☐ | Pending | Record model and firmware version |
| Linux x86_64 reference machine | UEFI | ☐ | ☐ | ☐ | ☐ | Pending | Record distro and firmware version |
| Additional supported machine | BIOS/UEFI | ☐ | ☐ | ☐ | ☐ | Pending | Add one row per device |

## Procedure

1. Record vendor, model, CPU, RAM, firmware mode/version and date.
2. Boot the standalone BIOS or UEFI image from removable media.
3. Confirm serial/framebuffer banner, keyboard input and `brane>` prompt.
4. If networking is available, run `net status` and record link state.
5. Run `acpi`, then `suspend`; verify wake and a post-resume shell command.
6. Attach serial logs and mark only the tested cells as ✅.

A physical result is not inferred from QEMU. Unsupported hardware or failed
ACPI behavior must remain visible in this matrix and in the release notes.
