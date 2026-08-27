#!/usr/bin/env python3
"""QEMU integration test for the ACPI S3 suspend/resume path."""

import argparse
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from qemu_serial import serial_lines


REPO_ROOT = Path(__file__).resolve().parents[2]
QEMU_BIN = os.environ.get("QEMU_BIN", "qemu-system-x86_64")
DEFAULT_TIMEOUT = int(os.environ.get("BOOT_TIMEOUT", "120"))
FAIL_STRINGS = ("KERNEL PANIC", "DOUBLE FAULT", "PAGE FAULT", "GENERAL PROTECTION FAULT")


def info(message: str) -> None:
    print(f"[acpi-s3]  \033[36mINFO\033[0m  {message}", flush=True)


def passed(message: str) -> None:
    print(f"[acpi-s3]  \033[32mPASS\033[0m  {message}", flush=True)


def failed(message: str) -> None:
    print(f"[acpi-s3]  \033[31mFAIL\033[0m  {message}", flush=True)


class QmpClient:
    def __init__(self, path: Path, deadline: float):
        while not path.exists():
            if time.monotonic() >= deadline:
                raise TimeoutError("QMP socket was not created")
            time.sleep(0.05)

        self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.socket.settimeout(1.0)
        self.socket.connect(str(path))
        self.stream = self.socket.makefile("rwb", buffering=0)
        self.events: list[dict] = []
        greeting = self._read_message(deadline)
        if "QMP" not in greeting:
            raise RuntimeError(f"invalid QMP greeting: {greeting}")
        self.execute("qmp_capabilities", deadline=deadline)

    def close(self) -> None:
        self.stream.close()
        self.socket.close()

    def _read_message(self, deadline: float) -> dict:
        while time.monotonic() < deadline:
            try:
                line = self.stream.readline()
            except TimeoutError:
                continue
            if not line:
                raise RuntimeError("QMP connection closed")
            return json.loads(line)
        raise TimeoutError("timed out waiting for QMP message")

    def execute(self, command: str, arguments: dict | None = None, deadline: float | None = None) -> dict:
        deadline = deadline or (time.monotonic() + 10)
        request_id = f"brane-{time.monotonic_ns()}"
        payload: dict = {"execute": command, "id": request_id}
        if arguments:
            payload["arguments"] = arguments
        self.stream.write((json.dumps(payload) + "\n").encode())

        while True:
            message = self._read_message(deadline)
            if "event" in message:
                self.events.append(message)
                continue
            if message.get("id") != request_id:
                continue
            if "error" in message:
                raise RuntimeError(f"QMP {command} failed: {message['error']}")
            return message

    def wait_event(self, name: str, deadline: float) -> dict:
        while True:
            for index, event in enumerate(self.events):
                if event.get("event") == name:
                    return self.events.pop(index)
            message = self._read_message(deadline)
            if "event" in message:
                self.events.append(message)


def send_text(qmp: QmpClient, text: str, deadline: float) -> None:
    key_names = {"\n": "ret", " ": "spc"}
    for character in text:
        key = key_names.get(character, character)
        qmp.execute(
            "human-monitor-command",
            {"command-line": f"sendkey {key}"},
            deadline,
        )
        time.sleep(0.06)


def run_test(image: Path, timeout: int) -> int:
    deadline = time.monotonic() + timeout
    shell_ready = threading.Event()
    resumed = threading.Event()
    acpi_status = threading.Event()
    failures: list[str] = []

    with tempfile.TemporaryDirectory(prefix="brane-acpi-") as temp_dir:
        qmp_path = Path(temp_dir) / "qmp.sock"
        command = [
            QEMU_BIN,
            "-m",
            "256M",
            "-drive",
            f"format=raw,file={image}",
            "-serial",
            "stdio",
            "-nographic",
            "-monitor",
            "none",
            "-qmp",
            f"unix:{qmp_path},server=on,wait=off",
            "-no-reboot",
            "-accel",
            "tcg",
        ]
        info(f"Launching QEMU (timeout={timeout}s)")
        proc = subprocess.Popen(
            command,
            cwd=REPO_ROOT,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )

        def read_serial() -> None:
            assert proc.stdout is not None
            for line in serial_lines(proc.stdout, ("brane>", *FAIL_STRINGS)):
                print(f"  serial │ {line}", flush=True)
                if "brane>" in line:
                    shell_ready.set()
                if "[acpi] Resume complete" in line:
                    resumed.set()
                if "ACPI initialized: true" in line:
                    acpi_status.set()
                for marker in FAIL_STRINGS:
                    if marker in line:
                        failures.append(marker)
                        resumed.set()

        reader = threading.Thread(target=read_serial, daemon=True)
        reader.start()
        qmp = None
        try:
            qmp = QmpClient(qmp_path, deadline)
            if not shell_ready.wait(max(0, deadline - time.monotonic())):
                raise TimeoutError("shell prompt did not appear")
            passed("Kernel booted with an interactive shell")
            shell_ready.clear()

            send_text(qmp, "suspend\n", deadline)
            qmp.wait_event("SUSPEND", deadline)
            passed("QEMU observed the guest entering ACPI S3")

            qmp.execute("system_wakeup", deadline=deadline)
            qmp.wait_event("WAKEUP", deadline)
            passed("QEMU delivered the wake event")

            if not resumed.wait(min(8, max(0, deadline - time.monotonic()))):
                registers = qmp.execute(
                    "human-monitor-command",
                    {"command-line": "info registers"},
                    deadline,
                ).get("return", "")
                low_memory = qmp.execute(
                    "human-monitor-command",
                    {"command-line": "xp /128bx 0x1000"},
                    deadline,
                ).get("return", "")
                info("CPU state after failed wake:\n" + registers)
                info("Wake page after failed wake:\n" + low_memory)
                raise TimeoutError("kernel did not report resume completion")
            if failures:
                raise RuntimeError(f"kernel failure after wake: {failures[0]}")
            passed("Kernel restored platform state after S3")

            if not shell_ready.wait(max(0, deadline - time.monotonic())):
                raise TimeoutError("shell prompt did not return after resume")
            shell_ready.clear()
            send_text(qmp, "acpi\n", deadline)
            if not acpi_status.wait(max(0, deadline - time.monotonic())):
                raise TimeoutError("shell did not execute a command after resume")
            passed("brsh accepted a command after resume")
            return 0
        except (OSError, RuntimeError, TimeoutError) as error:
            failed(str(error))
            return 1
        finally:
            if qmp is not None:
                qmp.close()
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
            reader.join(timeout=2)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--img", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT)
    args = parser.parse_args()
    if not args.img.exists():
        failed(f"disk image not found: {args.img}")
        return 2
    return run_test(args.img.resolve(), args.timeout)


if __name__ == "__main__":
    raise SystemExit(main())
