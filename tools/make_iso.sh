#!/usr/bin/env bash
# =============================================================================
# tools/make_iso.sh — Brane OS ISO packaging script
# =============================================================================
# Builds a bootable UEFI ISO image suitable for physical hardware or
# distribution via:
#   - QEMU: qemu-system-x86_64 -cdrom dist/brane_os_v<VERSION>.iso
#   - USB:  dd if=dist/brane_os_v<VERSION>.iso of=/dev/sdX bs=4M
#   - VMs:  Any x86 hypervisor (VirtualBox, VMware, Proxmox)
#
# Requirements (must be in PATH):
#   - cargo (Rust nightly)
#   - xorriso  ← apt install xorriso / brew install xorriso
# The ISO uses a standard UEFI El Torito image, so GRUB is not required.
#
# Usage:
#   ./tools/make_iso.sh [VERSION]
#
# Examples:
#   ./tools/make_iso.sh            # version from git tag or "dev"
#   ./tools/make_iso.sh 0.10.0     # explicit version
#   VERSION=0.10.0 ./tools/make_iso.sh
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
COLOR_RESET="\033[0m"
COLOR_CYAN="\033[36m"
COLOR_GREEN="\033[32m"
COLOR_RED="\033[31m"
COLOR_YELLOW="\033[33m"

info()  { echo -e "${COLOR_CYAN}[make_iso]${COLOR_RESET}  $*"; }
ok()    { echo -e "${COLOR_GREEN}[make_iso] ✓${COLOR_RESET} $*"; }
warn()  { echo -e "${COLOR_YELLOW}[make_iso] ⚠${COLOR_RESET}  $*"; }
die()   { echo -e "${COLOR_RED}[make_iso] ✗${COLOR_RESET}  $*" >&2; exit 1; }

require_cmd() {
    command -v "$1" &>/dev/null || die "Required command not found: $1 — please install it first."
}

write_sha256() {
    local file="$1"
    if command -v sha256sum &>/dev/null; then
        sha256sum "${file}"
    elif command -v shasum &>/dev/null; then
        shasum -a 256 "${file}"
    else
        die "Neither sha256sum nor shasum is available."
    fi
}

# ---------------------------------------------------------------------------
# Environment & paths
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

TARGET="x86_64-unknown-none"
KERNEL_BIN="${REPO_ROOT}/target/${TARGET}/release/brane_os_kernel"
RUNNER_OUT="${REPO_ROOT}/target/release-img"
BIOS_IMG="${RUNNER_OUT}/brane_os-bios.img"
UEFI_IMG="${RUNNER_OUT}/brane_os-uefi.img"

# Determine version
if [[ -n "${1:-}" ]]; then
    VERSION="$1"
elif [[ -n "${VERSION:-}" ]]; then
    : # already set via env
else
    VERSION="$(git -C "${REPO_ROOT}" describe --tags --always 2>/dev/null || echo "dev")"
fi

DIST_DIR="${REPO_ROOT}/dist"
ISO_NAME="brane_os_v${VERSION}.iso"
ISO_PATH="${DIST_DIR}/${ISO_NAME}"
ISO_ROOT="${REPO_ROOT}/target/iso_root"

# All Cargo workspace operations below are relative to the repository root.
cd "${REPO_ROOT}"

info "Brane OS ISO builder"
info "  Version  : ${VERSION}"
info "  Repo root: ${REPO_ROOT}"
info "  Output   : ${ISO_PATH}"
echo

# ---------------------------------------------------------------------------
# Dependency check
# ---------------------------------------------------------------------------
require_cmd cargo
require_cmd xorriso
require_cmd python3

# ---------------------------------------------------------------------------
# Step 1 — Build kernel (release)
# ---------------------------------------------------------------------------
info "[1/5] Building kernel (release)…"
cargo build -p brane_os_kernel \
    --target "${TARGET}" \
    --release \
    -Z build-std=core,compiler_builtins,alloc \
    -Z build-std-features=compiler-builtins-mem
ok "Kernel built: ${KERNEL_BIN}"

# ---------------------------------------------------------------------------
# Step 2 — Build BIOS disk image via runner
# ---------------------------------------------------------------------------
info "[2/5] Building BIOS disk image…"
mkdir -p "${RUNNER_OUT}"

# The runner uses OUT_DIR to know where to place the image
NO_RUN=1 KERNEL_BIN_PATH="${KERNEL_BIN}" OUT_DIR="${RUNNER_OUT}" \
    cargo run --package runner --release

# Locate the image (runner names it brane_os-bios.img inside OUT_DIR)
if [[ ! -f "${BIOS_IMG}" ]]; then
    # Fallback: find any .img in RUNNER_OUT
    BIOS_IMG="$(find "${RUNNER_OUT}" -name "*.img" | head -1)"
fi
[[ -n "${BIOS_IMG}" && -f "${BIOS_IMG}" ]] || die "BIOS image not found after runner step."
ok "BIOS image: ${BIOS_IMG}"

# ---------------------------------------------------------------------------
# Step 3 — Assemble ISO root filesystem
# ---------------------------------------------------------------------------
info "[3/5] Assembling ISO root…"
rm -rf "${ISO_ROOT}"
mkdir -p "${ISO_ROOT}/boot"

python3 "${SCRIPT_DIR}/extract_efi_partition.py" \
    "${UEFI_IMG}" "${ISO_ROOT}/boot/efi.img"

# Embed version metadata
cat > "${ISO_ROOT}/boot/brane_os.txt" <<EOF
Brane OS v${VERSION}
Built: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
Architecture: x86_64 (UEFI)
Boot media: UEFI El Torito (BIOS image available separately)
EOF

ok "ISO root assembled at: ${ISO_ROOT}"

# ---------------------------------------------------------------------------
# Step 4 — Generate ISO
# ---------------------------------------------------------------------------
info "[4/5] Generating ISO…"
mkdir -p "${DIST_DIR}"

# Register the extracted EFI System Partition as a no-emulation El Torito
# image. The standalone BIOS image remains available for legacy boot media.
xorriso -as mkisofs \
    -R -J \
    -V "BRANE_OS" \
    -e boot/efi.img \
    -no-emul-boot \
    -boot-load-size 4 \
    -o "${ISO_PATH}" \
    "${ISO_ROOT}"

ok "ISO created: ${ISO_PATH}"

# ---------------------------------------------------------------------------
# Step 5 — Checksums & release archive
# ---------------------------------------------------------------------------
info "[5/5] Generating checksums and release archive…"
cd "${DIST_DIR}"

# SHA256 checksum file
write_sha256 "${ISO_NAME}" > "${ISO_NAME}.sha256"
ok "Checksum: ${ISO_NAME}.sha256"

# Compressed tarball for GitHub Releases
ARCHIVE_NAME="brane_os_v${VERSION}_release.tar.gz"
BIOS_RELEASE_NAME="brane_os_v${VERSION}-bios.img"
UEFI_RELEASE_NAME="brane_os_v${VERSION}-uefi.img"
cp "${BIOS_IMG}" "${BIOS_RELEASE_NAME}"
cp "${UEFI_IMG}" "${UEFI_RELEASE_NAME}"
tar -czf "${ARCHIVE_NAME}" \
    "${ISO_NAME}" "${ISO_NAME}.sha256" \
    "${BIOS_RELEASE_NAME}" "${UEFI_RELEASE_NAME}"
ok "Release archive: ${ARCHIVE_NAME}"

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo
info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
ok  "Brane OS v${VERSION} release packaged successfully!"
info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo
echo "  ISO       → ${DIST_DIR}/${ISO_NAME}"
echo "  Checksum  → ${DIST_DIR}/${ISO_NAME}.sha256"
echo "  BIOS IMG  → ${DIST_DIR}/${BIOS_RELEASE_NAME}"
echo "  UEFI IMG  → ${DIST_DIR}/${UEFI_RELEASE_NAME}"
echo "  Archive   → ${DIST_DIR}/${ARCHIVE_NAME}"
echo
echo "  Quick test with QEMU:"
echo "    qemu-system-x86_64 -cdrom ${DIST_DIR}/${ISO_NAME} -m 256M -serial stdio -nographic"
echo
