#!/usr/bin/env bash
set -euo pipefail

ACTION="${1:-help}"
VM_NAME="${PARALLELS_VM_NAME:-Brane OS}"
ISO_PATH="${PARALLELS_ISO:-dist/brane_os_v${VERSION:-dev}.iso}"
CPUS="${PARALLELS_CPUS:-4}"
MEMORY_MB="${PARALLELS_MEMORY_MB:-512}"
START_AFTER_DEPLOY="${PARALLELS_START:-0}"

log() { printf '[parallels] %s\n' "$*"; }
fail() { printf '[parallels] ERROR: %s\n' "$*" >&2; exit 1; }

require_prlctl() {
    command -v prlctl >/dev/null 2>&1 || fail "prlctl not found; install Parallels Desktop first"
}

vm_exists() {
    prlctl list -a --output name | tail -n +2 | grep -Fqx "$VM_NAME"
}

vm_state() {
    prlctl status "$VM_NAME" 2>/dev/null | awk '{print $NF}'
}

require_iso() {
    [[ -f "$ISO_PATH" ]] || fail "ISO not found: $ISO_PATH (run make iso VERSION=<version>)"
    ISO_PATH="$(cd "$(dirname "$ISO_PATH")" && pwd)/$(basename "$ISO_PATH")"
}

configure_vm() {
    prlctl set "$VM_NAME" \
        --cpus "$CPUS" \
        --memsize "$MEMORY_MB" \
        --bios-type efi64 \
        --efi-secure-boot off \
        --startup-view window \
        --on-window-close keep-running >/dev/null

    if ! prlctl set "$VM_NAME" --device-set cdrom0 --image "$ISO_PATH" --connect >/dev/null 2>&1; then
        prlctl set "$VM_NAME" --device-add cdrom --image "$ISO_PATH" --connect >/dev/null
    fi
    prlctl set "$VM_NAME" --device-bootorder "cdrom0" >/dev/null
}

deploy() {
    require_iso
    if vm_exists; then
        local state
        state="$(vm_state)"
        [[ "$state" != "running" ]] || fail "VM '$VM_NAME' is running; stop it before replacing its ISO"
        log "updating existing VM '$VM_NAME'"
    else
        log "creating dedicated VM '$VM_NAME'"
        prlctl create "$VM_NAME" -o other --no-hdd >/dev/null
    fi
    configure_vm
    log "ready: VM='$VM_NAME', ISO='$ISO_PATH', CPUs=$CPUS, RAM=${MEMORY_MB}MB"
    if [[ "$START_AFTER_DEPLOY" == "1" ]]; then
        prlctl start "$VM_NAME"
    else
        log "start with: PARALLELS_VM_NAME='$VM_NAME' make parallels-start"
    fi
}

require_prlctl
case "$ACTION" in
    deploy) deploy ;;
    start)
        vm_exists || fail "VM '$VM_NAME' does not exist; run make parallels-deploy first"
        prlctl start "$VM_NAME"
        ;;
    stop)
        vm_exists || fail "VM '$VM_NAME' does not exist"
        prlctl stop "$VM_NAME" --acpi
        ;;
    status)
        vm_exists || fail "VM '$VM_NAME' does not exist"
        prlctl list -i "$VM_NAME"
        ;;
    help|*)
        cat <<USAGE
Usage: tools/parallels_deploy.sh <deploy|start|stop|status>

Environment:
  VERSION=dev                    artifact version used by the default ISO path
  PARALLELS_VM_NAME="Brane OS"   dedicated VM name
  PARALLELS_ISO=/path/brane.iso  ISO to attach
  PARALLELS_CPUS=4               virtual CPUs
  PARALLELS_MEMORY_MB=512        memory in MiB
  PARALLELS_START=1              start after deploy
USAGE
        ;;
esac
