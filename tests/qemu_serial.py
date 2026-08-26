"""Shared serial-stream helpers for Brane OS QEMU test harnesses."""

from typing import Iterator, TextIO


def serial_lines(
    stream: TextIO,
    flush_markers: tuple[str, ...] = (),
) -> Iterator[str]:
    """Yield serial output without waiting forever for a trailing newline.

    The Brane shell renders ``brane>`` without a newline. Reading QEMU stdout
    with normal line iteration therefore blocks until QEMU exits. Flush a
    partial line as soon as one of ``flush_markers`` is observed so callers can
    detect prompts and fatal conditions immediately.
    """

    line_chars: list[str] = []
    scan_window = ""
    max_marker_len = max((len(marker) for marker in flush_markers), default=0)

    while char := stream.read(1):
        line_chars.append(char)

        if max_marker_len:
            scan_window = (scan_window + char)[-max_marker_len:]

        marker_found = any(marker in scan_window for marker in flush_markers)
        if char == "\n" or marker_found:
            yield "".join(line_chars).rstrip("\r\n")
            line_chars.clear()
            scan_window = ""

    if line_chars:
        yield "".join(line_chars).rstrip("\r\n")
