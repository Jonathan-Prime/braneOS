#!/usr/bin/env python3
"""Validate the files produced by tools/make_iso.sh."""

import argparse
import hashlib
import subprocess
import tarfile
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(f"[release-test] FAIL: {message}")


def require_file(path: Path) -> None:
    if not path.is_file() or path.stat().st_size == 0:
        fail(f"missing or empty artifact: {path}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate Brane OS release artifacts")
    parser.add_argument("--dist", type=Path, default=Path("dist"))
    parser.add_argument("--version", default="dev")
    args = parser.parse_args()

    prefix = f"brane_os_v{args.version}"
    iso = args.dist / f"{prefix}.iso"
    checksum = args.dist / f"{prefix}.iso.sha256"
    bios = args.dist / f"{prefix}-bios.img"
    uefi = args.dist / f"{prefix}-uefi.img"
    archive = args.dist / f"{prefix}_release.tar.gz"
    for artifact in (iso, checksum, bios, uefi, archive):
        require_file(artifact)

    expected_hash = checksum.read_text(encoding="utf-8").split()[0]
    actual_hash = hashlib.sha256(iso.read_bytes()).hexdigest()
    if expected_hash != actual_hash:
        fail(f"checksum mismatch for {iso.name}")

    with tarfile.open(archive, "r:gz") as bundle:
        members = {member.name for member in bundle.getmembers()}
    expected_members = {iso.name, checksum.name, bios.name, uefi.name}
    if not expected_members.issubset(members):
        fail(f"archive missing: {sorted(expected_members - members)}")

    if iso.read_bytes()[0x8001:0x8006] != b"CD001":
        fail(f"{iso.name} is not an ISO 9660 image")
    report = subprocess.run(
        ["xorriso", "-indev", str(iso), "-report_el_torito", "plain"],
        capture_output=True,
        text=True,
        check=False,
    )
    if report.returncode != 0 or "UEFI" not in report.stdout:
        fail("ISO does not advertise a UEFI El Torito boot image")

    print(f"[release-test] PASS: {prefix} artifacts, checksum, archive and UEFI catalog")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
