#!/usr/bin/env bash
# ============================================================
# Brane OS — QEMU Runner
# ============================================================
# Usage: ./run.sh [kernel_binary]
#
# Launches qemu-system-x86_64 with:
#   - Serial output to stdio
#   - No graphical display (nographic)
#   - 128 MB RAM (suitable for early kernel)
#   - Debug interrupt exits enabled
# ============================================================

set -euo pipefail

KERNEL="${1:?Usage: $0 <kernel_binary>}"

if [ ! -f "$KERNEL" ]; then
    echo "Error: Kernel binary not found: $KERNEL" >&2
    exit 1
fi

QEMU="qemu-system-x86_64"

# Check if QEMU is available
if ! command -v "$QEMU" &> /dev/null; then
    echo "Error: $QEMU not found. Please install QEMU." >&2
    echo "  macOS:  brew install qemu" >&2
    echo "  Linux:  sudo apt install qemu-system-x86" >&2
    exit 1
fi

exec "$QEMU" \
    -kernel "$KERNEL" \
    -serial stdio \
    -display none \
    -m 128M \
    -no-reboot \
    -d int,cpu_reset \
    -D /tmp/brane_os_qemu.log \
    "$@"
