#!/usr/bin/env bash
# =============================================================================
# tools/make_iso.sh — Brane OS ISO packaging script
# =============================================================================
# Builds a booteable BIOS ISO image suitable for physical hardware or
# distribution via:
#   - QEMU: qemu-system-x86_64 -cdrom dist/brane_os_v<VERSION>.iso
#   - USB:  dd if=dist/brane_os_v<VERSION>.iso of=/dev/sdX bs=4M
#   - VMs:  Any x86 hypervisor (VirtualBox, VMware, Proxmox)
#
# Requirements (must be in PATH):
#   - cargo (Rust nightly)
#   - xorriso  ← apt install xorriso / brew install xorriso
#   - grub-mkimage / grub-mkrescue  ← apt install grub-pc-bin grub-common
#     OR just xorriso + provided grub.cfg (El Torito method below)
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

# ---------------------------------------------------------------------------
# Environment & paths
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

TARGET="x86_64-unknown-none"
KERNEL_BIN="${REPO_ROOT}/target/${TARGET}/release/brane_os_kernel"
RUNNER_OUT="${REPO_ROOT}/target/release-img"
BIOS_IMG="${RUNNER_OUT}/brane_os-bios.img"

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
GRUB_CFG="${SCRIPT_DIR}/grub.cfg"

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

# grub-mkrescue is optional if we use xorriso El Torito directly
USE_GRUB_MKRESCUE=false
if command -v grub-mkrescue &>/dev/null; then
    USE_GRUB_MKRESCUE=true
elif command -v grub2-mkrescue &>/dev/null; then
    USE_GRUB_MKRESCUE=true
    shopt -s expand_aliases
    alias grub-mkrescue=grub2-mkrescue
fi

# ---------------------------------------------------------------------------
# Step 1 — Build kernel (release)
# ---------------------------------------------------------------------------
info "[1/5] Building kernel (release)…"
cargo build -p brane_os_kernel \
    --target "${TARGET}" \
    --release \
    -Z build-std=core,compiler_builtins,alloc \
    -Z build-std-features=compiler-builtins-mem \
    -C "${REPO_ROOT}"
ok "Kernel built: ${KERNEL_BIN}"

# ---------------------------------------------------------------------------
# Step 2 — Build BIOS disk image via runner
# ---------------------------------------------------------------------------
info "[2/5] Building BIOS disk image…"
mkdir -p "${RUNNER_OUT}"

# The runner uses OUT_DIR to know where to place the image
KERNEL_BIN_PATH="${KERNEL_BIN}" OUT_DIR="${RUNNER_OUT}" \
    cargo run --package runner --release -C "${REPO_ROOT}"

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
mkdir -p "${ISO_ROOT}/boot/grub"

cp "${BIOS_IMG}"   "${ISO_ROOT}/boot/brane_os.img"
cp "${GRUB_CFG}"   "${ISO_ROOT}/boot/grub/grub.cfg"

# Embed version metadata
cat > "${ISO_ROOT}/boot/brane_os.txt" <<EOF
Brane OS v${VERSION}
Built: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
Architecture: x86_64 (BIOS)
EOF

ok "ISO root assembled at: ${ISO_ROOT}"

# ---------------------------------------------------------------------------
# Step 4 — Generate ISO
# ---------------------------------------------------------------------------
info "[4/5] Generating ISO…"
mkdir -p "${DIST_DIR}"

if $USE_GRUB_MKRESCUE; then
    info "Using grub-mkrescue…"
    grub-mkrescue \
        --output="${ISO_PATH}" \
        "${ISO_ROOT}"
else
    info "grub-mkrescue not found — using xorriso El Torito (BIOS raw)…"
    warn "For GRUB menu support, install grub-pc-bin and grub-common."
    # Embed the raw BIOS image directly as El Torito boot catalog
    xorriso -as mkisofs \
        -R -J \
        -V "BRANE_OS_${VERSION}" \
        -b boot/brane_os.img \
        -no-emul-boot \
        -boot-load-size 4 \
        -boot-info-table \
        -o "${ISO_PATH}" \
        "${ISO_ROOT}"
fi

ok "ISO created: ${ISO_PATH}"

# ---------------------------------------------------------------------------
# Step 5 — Checksums & release archive
# ---------------------------------------------------------------------------
info "[5/5] Generating checksums and release archive…"
cd "${DIST_DIR}"

# SHA256 checksum file
sha256sum "${ISO_NAME}" > "${ISO_NAME}.sha256"
ok "Checksum: ${ISO_NAME}.sha256"

# Compressed tarball for GitHub Releases
ARCHIVE_NAME="brane_os_v${VERSION}_release.tar.gz"
tar -czf "${ARCHIVE_NAME}" "${ISO_NAME}" "${ISO_NAME}.sha256"
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
echo "  Archive   → ${DIST_DIR}/${ARCHIVE_NAME}"
echo
echo "  Quick test with QEMU:"
echo "    qemu-system-x86_64 -cdrom ${DIST_DIR}/${ISO_NAME} -m 256M -serial stdio -nographic"
echo
