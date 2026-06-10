#!/usr/bin/env python3
"""
tests/boot/test_boot.py — Brane OS Boot Test Harness
=====================================================
Launches Brane OS inside QEMU and verifies that the kernel boots
successfully by inspecting serial output within a configurable timeout.

Exit codes:
  0 — PASS: all expected strings were found in serial output
  1 — FAIL: timeout or expected string not found
  2 — ERROR: build or QEMU setup failed

Usage:
  python3 tests/boot/test_boot.py [--timeout SECONDS] [--no-build]

Environment variables:
  KERNEL_BIN_PATH  — path to pre-built kernel binary (skips cargo build)
  BOOT_TIMEOUT     — override timeout in seconds (default: 60)
"""

import argparse
import os
import re
import subprocess
import sys
import threading
import time
from pathlib import Path

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parents[2]
KERNEL_CRATE = "brane_os_kernel"
TARGET = "x86_64-unknown-none"
BUILD_FLAGS = [
    "-Z", "build-std=core,compiler_builtins,alloc",
    "-Z", "build-std-features=compiler-builtins-mem",
    "--target", TARGET,
]

DEFAULT_TIMEOUT = int(os.environ.get("BOOT_TIMEOUT", "60"))

# Strings that MUST appear in the serial output for the test to pass.
# Order does not matter; all must be present.
REQUIRED_STRINGS = [
    "Brane OS",   # kernel banner
    "brane>",     # brsh prompt (signals full userland init)
]

# Strings whose presence immediately fails the test (kernel panics, etc.)
FAIL_STRINGS = [
    "KERNEL PANIC",
    "panicked at",
    "DOUBLE FAULT",
    "STACK OVERFLOW",
]

QEMU_BIN = os.environ.get("QEMU_BIN", "qemu-system-x86_64")

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def info(msg: str) -> None:
    print(f"[boot-test] \033[36mINFO\033[0m  {msg}", flush=True)

def ok(msg: str) -> None:
    print(f"[boot-test] \033[32mPASS\033[0m  {msg}", flush=True)

def warn(msg: str) -> None:
    print(f"[boot-test] \033[33mWARN\033[0m  {msg}", flush=True)

def error(msg: str) -> None:
    print(f"[boot-test] \033[31mFAIL\033[0m  {msg}", flush=True)

# ---------------------------------------------------------------------------
# Build step
# ---------------------------------------------------------------------------

def build_kernel() -> Path:
    """Build the kernel (debug) and return the binary path."""
    info("Building kernel (debug)…")
    result = subprocess.run(
        ["cargo", "build", "-p", KERNEL_CRATE, "--target", TARGET,
         "-Z", "build-std=core,compiler_builtins,alloc",
         "-Z", "build-std-features=compiler-builtins-mem"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        error("cargo build failed:")
        print(result.stderr, file=sys.stderr)
        sys.exit(2)
    bin_path = REPO_ROOT / "target" / TARGET / "debug" / "brane_os_kernel"
    if not bin_path.exists():
        error(f"Kernel binary not found at: {bin_path}")
        sys.exit(2)
    info(f"Kernel binary: {bin_path}")
    return bin_path


def build_disk_image(kernel_path: Path) -> Path:
    """Use the runner crate to produce a BIOS disk image."""
    info("Building BIOS disk image…")
    out_dir = REPO_ROOT / "target" / "boot-test-img"
    out_dir.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "KERNEL_BIN_PATH": str(kernel_path)}
    result = subprocess.run(
        ["cargo", "run", "--package", "runner"],
        cwd=REPO_ROOT,
        env=env,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        error("runner failed to create disk image:")
        print(result.stderr, file=sys.stderr)
        sys.exit(2)

    # The runner places images in $OUT_DIR which defaults to target/
    candidates = list((REPO_ROOT / "target").glob("**/*.img"))
    bios_imgs = [p for p in candidates if "bios" in p.name]
    if not bios_imgs:
        # Fallback: any .img found
        bios_imgs = candidates
    if not bios_imgs:
        error("No .img file produced by runner.")
        sys.exit(2)
    # Pick the most recently modified one
    img = max(bios_imgs, key=lambda p: p.stat().st_mtime)
    info(f"Disk image: {img}")
    return img

# ---------------------------------------------------------------------------
# QEMU runner
# ---------------------------------------------------------------------------

def run_qemu_test(img_path: Path, timeout: int) -> int:
    """
    Launch QEMU with the given image, capture serial output,
    and return 0 (pass), 1 (fail).
    """
    cmd = [
        QEMU_BIN,
        "-m", "256M",
        "-drive", f"format=raw,file={img_path}",
        "-serial", "stdio",
        "-nographic",         # no display window — pure serial I/O
        "-no-reboot",         # exit on triple fault instead of rebooting
        "-accel", "tcg",      # software emulation — works in any CI/CD
    ]
    info(f"Launching QEMU (timeout={timeout}s):")
    info("  " + " ".join(cmd))

    found = set()
    failed_reason: list[str] = []
    output_lines: list[str] = []
    done_event = threading.Event()

    try:
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )
    except FileNotFoundError:
        error(f"{QEMU_BIN} not found. Install QEMU and ensure it is in PATH.")
        return 2

    def reader():
        assert proc.stdout is not None
        try:
            for raw_line in proc.stdout:
                line = raw_line.rstrip()
                output_lines.append(line)
                print(f"  serial │ {line}", flush=True)

                for req in REQUIRED_STRINGS:
                    if req in line:
                        found.add(req)
                        ok(f"Found required string: {req!r}")

                for bad in FAIL_STRINGS:
                    if bad in line:
                        failed_reason.append(f"Detected failure string: {bad!r}")

                # If all required strings found and no failures, we're done
                if found == set(REQUIRED_STRINGS) and not failed_reason:
                    done_event.set()
        except Exception:
            pass
        finally:
            done_event.set()

    t = threading.Thread(target=reader, daemon=True)
    t.start()

    # Wait for success signal or timeout
    triggered = done_event.wait(timeout=timeout)

    # Terminate QEMU
    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()

    t.join(timeout=3)

    # ── Evaluate result ──────────────────────────────────────────────────
    print()
    if failed_reason:
        for msg in failed_reason:
            error(msg)
        return 1

    missing = set(REQUIRED_STRINGS) - found
    if missing:
        if not triggered:
            error(f"Timeout after {timeout}s — serial output did not contain all required strings.")
        else:
            error("QEMU exited before all required strings were found.")
        error(f"Missing: {[s for s in missing]}")
        info("--- Captured serial output ---")
        for ln in output_lines:
            print(f"  {ln}")
        return 1

    ok("All required strings found in serial output. Boot test PASSED ✓")
    return 0

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Brane OS Boot Test")
    p.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT,
                   help=f"Seconds before giving up (default: {DEFAULT_TIMEOUT})")
    p.add_argument("--no-build", action="store_true",
                   help="Skip build step; use KERNEL_BIN_PATH env var or pre-existing image")
    p.add_argument("--img", type=str, default=None,
                   help="Path to existing .img file, skips build entirely")
    return p.parse_args()


def main() -> None:
    args = parse_args()
    start = time.monotonic()

    info(f"Brane OS Boot Test — timeout={args.timeout}s")
    info(f"Repo root: {REPO_ROOT}")

    if args.img:
        img_path = Path(args.img)
        if not img_path.exists():
            error(f"Image not found: {img_path}")
            sys.exit(2)
    elif args.no_build and os.environ.get("KERNEL_BIN_PATH"):
        kernel_path = Path(os.environ["KERNEL_BIN_PATH"])
        img_path = build_disk_image(kernel_path)
    else:
        kernel_path = build_kernel()
        img_path = build_disk_image(kernel_path)

    rc = run_qemu_test(img_path, args.timeout)

    elapsed = time.monotonic() - start
    info(f"Total elapsed: {elapsed:.1f}s")
    sys.exit(rc)


if __name__ == "__main__":
    main()
