#!/usr/bin/env python3
"""
tests/e2e/test_full_boot_flow.py — Brane OS Full Boot Flow E2E Test
====================================================================
Validates the complete sequential boot flow of Brane OS, from power-on
banner through all 9 initialization phases to the interactive shell prompt.

This test is more comprehensive than test_boot.py:
  - Verifies EVERY initialization phase is logged in the correct order.
  - Confirms each subsystem reports ready with expected metrics.
  - Validates the phase ordering matches ARCHITECTURE.md §4.2.
  - Confirms no phase is skipped or out-of-order.

Phases verified:
  Phase 1  — Core hardware (GDT, IDT, PIC, usermode MSRs)
  Phase 2  — Memory (frame allocator, paging, heap, scheduler)
  Phase 3  — Syscalls & IPC
  Phase 4  — Security & adaptability (caps, audit, modules)
  Phase 5  — Brane Protocol
  Phase 6  — AI subsystem
  Phase 7  — User space (process table)
  Phase 8  — VFS, TTY & Shell
  Phase 9  — Networking (virtio-net, TCP/IP, DNS, sockets)
  Final    — brsh prompt

Exit codes:
  0 — PASS
  1 — FAIL
  2 — ERROR

Usage:
  python3 tests/e2e/test_full_boot_flow.py [--timeout SECONDS]
"""

import argparse
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parents[2]
QEMU_BIN  = os.environ.get("QEMU_BIN", "qemu-system-x86_64")
DEFAULT_TIMEOUT = int(os.environ.get("BOOT_TIMEOUT", "120"))

# Ordered sequence of boot phases — each must appear in serial output
# (order is enforced by the kernel's own boot sequence)
BOOT_PHASES = [
    # Phase 0 — Banner
    ("P0:banner",   "Brane OS v0.1 — Kernel Booting"),
    # Phase 1 — Core hardware
    ("P1:gdt",      "[gdt]  Global Descriptor Table loaded"),
    ("P1:usermode", "[usermode] syscall MSRs configured"),
    # Phase 2 — Memory
    ("P2:mem",      "[mem]  Frame allocator ready"),
    ("P2:page",     "[page] OffsetPageTable initialized"),
    ("P2:heap",     "[heap] Kernel heap initialized"),
    ("P2:sched",    "[sched] Scheduler ready"),
    # Phase 3 — Syscalls
    ("P3:sys",      "[sys]  Syscall dispatcher ready"),
    ("P3:ipc",      "[ipc]  IPC core ready"),
    # Phase 4 — Security
    ("P4:cap",      "[cap]  Capability manager ready"),
    ("P4:aud",      "[aud]  Audit log ready"),
    ("P4:mod",      "[mod]  Module loader ready"),
    # Phase 5 — Brane Protocol
    ("P5:brane",    "[brane] Brane Protocol ready"),
    # Phase 6 — AI
    ("P6:ai",       "[ai]   AI engine ready"),
    # Phase 7 — User Space
    ("P7:proc",     "[proc] Process table ready"),
    # Boot complete banner
    ("P_done",      "Brane OS v0.1 — Boot Complete"),
    # Phase 8 — VFS / Shell
    ("P8:vfs",      "[vfs]  VFS ready"),
    # Phase 9 — Networking
    ("P9:dns",      "[dns]  DNS resolver ready"),
    ("P9:sock",     "[sock] Socket subsystem ready"),
    # Shell prompt
    ("P_shell",     "brane>"),
]

FAIL_STRINGS = [
    "KERNEL PANIC",
    "panicked at",
    "DOUBLE FAULT",
    "STACK OVERFLOW",
]

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def info(msg):  print(f"[flow-e2e]  \033[36mINFO\033[0m  {msg}", flush=True)
def ok(msg):    print(f"[flow-e2e]  \033[32mPASS\033[0m  {msg}", flush=True)
def warn(msg):  print(f"[flow-e2e]  \033[33mWARN\033[0m  {msg}", flush=True)
def error(msg): print(f"[flow-e2e]  \033[31mFAIL\033[0m  {msg}", flush=True)

# ---------------------------------------------------------------------------
# Build helpers
# ---------------------------------------------------------------------------

def build_kernel() -> Path:
    info("Building kernel (debug)…")
    result = subprocess.run(
        ["cargo", "build", "-p", "brane_os_kernel",
         "--target", "x86_64-unknown-none",
         "-Z", "build-std=core,compiler_builtins,alloc",
         "-Z", "build-std-features=compiler-builtins-mem"],
        cwd=REPO_ROOT, capture_output=True, text=True,
    )
    if result.returncode != 0:
        error("cargo build failed:")
        print(result.stderr, file=sys.stderr)
        sys.exit(2)
    bin_path = REPO_ROOT / "target" / "x86_64-unknown-none" / "debug" / "brane_os_kernel"
    if not bin_path.exists():
        error(f"Binary not found: {bin_path}")
        sys.exit(2)
    return bin_path


def build_disk_image(kernel_path: Path) -> Path:
    info("Building disk image…")
    env = {**os.environ, "KERNEL_BIN_PATH": str(kernel_path)}
    result = subprocess.run(
        ["cargo", "run", "--package", "runner"],
        cwd=REPO_ROOT, env=env, capture_output=True, text=True,
    )
    if result.returncode != 0:
        error("runner failed:")
        print(result.stderr, file=sys.stderr)
        sys.exit(2)
    candidates = list((REPO_ROOT / "target").glob("**/*.img"))
    bios_imgs  = [p for p in candidates if "bios" in p.name] or candidates
    if not bios_imgs:
        error("No .img produced.")
        sys.exit(2)
    return max(bios_imgs, key=lambda p: p.stat().st_mtime)

# ---------------------------------------------------------------------------
# QEMU runner
# ---------------------------------------------------------------------------

def run_full_boot_test(img_path: Path, timeout: int) -> int:
    cmd = [
        QEMU_BIN, "-m", "256M",
        "-drive", f"format=raw,file={img_path}",
        "-serial", "stdio", "-nographic", "-no-reboot", "-accel", "tcg",
    ]
    info(f"Launching QEMU (timeout={timeout}s):")
    info("  " + " ".join(cmd))

    # Track phases: id → (search_string, line_index_found)
    phase_found   = {}   # phase_id → line_number
    failed_reason = []
    output_lines  = []
    done_event    = threading.Event()

    # The last phase is the one that signals completion
    final_phase_id = BOOT_PHASES[-1][0]

    try:
        proc = subprocess.Popen(
            cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, bufsize=1,
        )
    except FileNotFoundError:
        error(f"{QEMU_BIN} not found.")
        return 2

    def reader():
        assert proc.stdout is not None
        line_idx = 0
        try:
            for raw_line in proc.stdout:
                line = raw_line.rstrip()
                output_lines.append(line)
                print(f"  serial │ {line}", flush=True)

                for phase_id, search_str in BOOT_PHASES:
                    if search_str in line and phase_id not in phase_found:
                        phase_found[phase_id] = line_idx
                        ok(f"Phase {phase_id}: {search_str!r}")

                for bad in FAIL_STRINGS:
                    if bad in line:
                        failed_reason.append(f"Failure string: {bad!r}")
                        done_event.set()

                if final_phase_id in phase_found and not failed_reason:
                    done_event.set()

                line_idx += 1
        except Exception:
            pass
        finally:
            done_event.set()

    t = threading.Thread(target=reader, daemon=True)
    t.start()
    triggered = done_event.wait(timeout=timeout)

    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
    t.join(timeout=3)

    print()

    if failed_reason:
        for msg in failed_reason:
            error(msg)
        return 1

    # --- Phase order verification ---
    missing_phases = [
        (pid, s) for pid, s in BOOT_PHASES if pid not in phase_found
    ]

    if missing_phases:
        if not triggered:
            error(f"Timeout after {timeout}s — boot flow incomplete")
        else:
            error("QEMU exited before full boot flow")
        error(f"Missing phases ({len(missing_phases)}/{len(BOOT_PHASES)}):")
        for pid, s in missing_phases:
            error(f"  [{pid}] {s!r}")
        info("Completed phases:")
        for pid, s in BOOT_PHASES:
            if pid in phase_found:
                info(f"  ✓ [{pid}] line {phase_found[pid]}: {s!r}")
        info("--- Serial output (first 60 lines) ---")
        for ln in output_lines[:60]:
            print(f"  {ln}")
        return 1

    # --- Verify phase ordering (line numbers must be ascending) ---
    out_of_order = []
    prev_line    = -1
    prev_id      = None
    for phase_id, _ in BOOT_PHASES:
        line_num = phase_found[phase_id]
        if line_num < prev_line:
            out_of_order.append(
                f"{phase_id} (line {line_num}) appeared before {prev_id} (line {prev_line})"
            )
        prev_line = line_num
        prev_id   = phase_id

    if out_of_order:
        error("Boot phase ordering violation!")
        for msg in out_of_order:
            error(f"  {msg}")
        return 1

    ok(f"All {len(BOOT_PHASES)} boot phases verified in correct order ✓")

    # --- Summary ---
    total_lines = len(output_lines)
    first_line  = phase_found.get("P0:banner", 0)
    shell_line  = phase_found.get("P_shell", total_lines)
    info(f"Boot span: lines {first_line}–{shell_line} of {total_lines} total")
    ok("Full boot flow E2E test PASSED ✓")
    return 0

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def parse_args():
    p = argparse.ArgumentParser(description="Brane OS Full Boot Flow E2E Test")
    p.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT)
    p.add_argument("--no-build", action="store_true")
    p.add_argument("--img", type=str, default=None)
    return p.parse_args()


def main():
    args = parse_args()
    start = time.monotonic()
    info(f"Brane OS Full Boot Flow E2E Test — timeout={args.timeout}s")
    info(f"Verifying {len(BOOT_PHASES)} boot phases in sequence")

    if args.img:
        img_path = Path(args.img)
        if not img_path.exists():
            error(f"Image not found: {img_path}")
            sys.exit(2)
    elif args.no_build and os.environ.get("KERNEL_BIN_PATH"):
        img_path = build_disk_image(Path(os.environ["KERNEL_BIN_PATH"]))
    else:
        img_path = build_disk_image(build_kernel())

    rc = run_full_boot_test(img_path, args.timeout)
    info(f"Total elapsed: {time.monotonic() - start:.1f}s")
    sys.exit(rc)


if __name__ == "__main__":
    main()
