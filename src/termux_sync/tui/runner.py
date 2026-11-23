"""Run the engine as a child process and turn its JSON Lines stdout into events."""

import json
import os
import signal
import subprocess
import threading
import time
from collections import deque
from typing import Callable

MAX_LINE = 65536
MAX_STDERR = 200


class OperationRunner:
    """Owns one engine child process.

    The engine is always started in its own session so a cancel can signal the
    whole group without touching the TUI.
    """

    def __init__(
        self,
        cli_prefix: list[str],
        on_event: Callable[[dict], None],
        on_exit: Callable[[int], None],
    ):
        self.cli_prefix = list(cli_prefix)
        self.on_event = on_event
        self.on_exit = on_exit
        self.stderr_tail: deque[str] = deque(maxlen=MAX_STDERR)
        self.proc: subprocess.Popen | None = None
        self._threads: list[threading.Thread] = []
        self._stdout_thread: threading.Thread | None = None
        self._stderr_thread: threading.Thread | None = None

    @property
    def pid(self) -> int | None:
        return self.proc.pid if self.proc is not None else None

    def start(self, command: str, name: str = "") -> None:
        argv = self.cli_prefix + [command, "--json-events"]
        if name:
            argv += ["--name", name]
        self.proc = subprocess.Popen(
            argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        self._stdout_thread = threading.Thread(target=self._read_stdout, daemon=True)
        self._stderr_thread = threading.Thread(target=self._read_stderr, daemon=True)
        waiter = threading.Thread(target=self._wait_and_report, daemon=True)
        self._threads = [self._stdout_thread, self._stderr_thread, waiter]
        for thread in self._threads:
            thread.start()

    def _warn(self, message: str) -> None:
        self.on_event(
            {
                "v": 1,
                "event": "log",
                "ts": int(time.time() * 1000),
                "level": "WARNING",
                "message": message,
            }
        )

    def _read_stdout(self) -> None:
        assert self.proc is not None and self.proc.stdout is not None
        stream = self.proc.stdout
        while True:
            line = stream.readline(MAX_LINE)
            if not line:
                break
            if not line.endswith(b"\n"):
                # Either oversized or a final partial line: drop the rest of it.
                while True:
                    rest = stream.readline(MAX_LINE)
                    if not rest or rest.endswith(b"\n"):
                        break
                self._warn(f"discarded event line longer than {MAX_LINE} bytes")
                continue
            text = line.decode("utf-8", errors="replace").strip()
            if not text:
                continue
            try:
                event = json.loads(text)
            except json.JSONDecodeError:
                self._warn(f"discarded malformed event line: {text[:120]}")
                continue
            if isinstance(event, dict):
                self.on_event(event)

    def _read_stderr(self) -> None:
        assert self.proc is not None and self.proc.stderr is not None
        for raw in self.proc.stderr:
            self.stderr_tail.append(raw.decode("utf-8", errors="replace").rstrip())

    def _wait_and_report(self) -> None:
        assert self.proc is not None
        code = self.proc.wait()
        if self._stdout_thread is not None:
            self._stdout_thread.join()
        if self._stderr_thread is not None:
            self._stderr_thread.join()
        self.on_exit(_exit_code(code))

    def confirm(self, text: str = "RESTORE") -> None:
        if self.proc is None or self.proc.stdin is None:
            return
        self.proc.stdin.write((json.dumps({"confirm": text}) + "\n").encode())
        self.proc.stdin.flush()

    def cancel(self) -> None:
        self._signal(signal.SIGINT)

    def kill(self) -> None:
        self._signal(signal.SIGKILL)

    def _signal(self, sig: int) -> None:
        if self.proc is None or self.proc.poll() is not None:
            return
        try:
            os.killpg(os.getpgid(self.proc.pid), sig)
        except (ProcessLookupError, PermissionError):
            pass

    def wait(self, timeout: float = 10.0) -> int:
        if self.proc is None:
            return 0
        try:
            return _exit_code(self.proc.wait(timeout=timeout))
        except subprocess.TimeoutExpired:
            self.kill()
            return _exit_code(self.proc.wait(timeout=timeout))

    def close(self) -> None:
        if self.proc is not None and self.proc.poll() is None:
            self.kill()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                pass
        for thread in self._threads:
            thread.join(timeout=5)
        if self.proc is not None:
            for stream in (self.proc.stdin, self.proc.stdout, self.proc.stderr):
                if stream is not None:
                    try:
                        stream.close()
                    except OSError:
                        pass


def _exit_code(code: int) -> int:
    return 128 - code if code < 0 else code
