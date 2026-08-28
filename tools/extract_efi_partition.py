#!/usr/bin/env python3
"""Extract the EFI System Partition from a bootloader GPT disk image."""

import argparse
import struct
from pathlib import Path

SECTOR_SIZE = 512
EFI_SYSTEM_PARTITION_GUID = bytes.fromhex("28732ac11ff8d211ba4b00a0c93ec93b")


def extract(source: Path, output: Path) -> None:
    with source.open("rb") as image:
        image.seek(SECTOR_SIZE)
        header = image.read(SECTOR_SIZE)
        if header[:8] != b"EFI PART":
            raise ValueError(f"{source} does not contain a GPT header")
        entries_lba = struct.unpack_from("<Q", header, 72)[0]
        entry_count = struct.unpack_from("<I", header, 80)[0]
        entry_size = struct.unpack_from("<I", header, 84)[0]
        if not 128 <= entry_size <= 4096:
            raise ValueError(f"unsupported GPT entry size: {entry_size}")

        image.seek(entries_lba * SECTOR_SIZE)
        efi_entry = None
        for _ in range(entry_count):
            entry = image.read(entry_size)
            if len(entry) != entry_size:
                break
            if entry[:16] == EFI_SYSTEM_PARTITION_GUID:
                efi_entry = entry
                break
        if efi_entry is None:
            raise ValueError("EFI System Partition not found")

        first_lba, last_lba = struct.unpack_from("<QQ", efi_entry, 32)
        if last_lba < first_lba:
            raise ValueError("EFI partition has invalid LBA range")
        length = (last_lba - first_lba + 1) * SECTOR_SIZE
        image.seek(first_lba * SECTOR_SIZE)
        output.parent.mkdir(parents=True, exist_ok=True)
        with output.open("wb") as extracted:
            remaining = length
            while remaining:
                chunk = image.read(min(1024 * 1024, remaining))
                if not chunk:
                    raise ValueError("EFI partition extends beyond disk image")
                extracted.write(chunk)
                remaining -= len(chunk)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        extract(args.source, args.output)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
