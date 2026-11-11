"""Subprocess containment within the existing Boxoro worker boundary."""

import os
import resource
import selectors
import signal
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path


@dataclass
class ProcessResult:
    status: str
    returncode: int | None
    stdout: str
    stderr: str
    duration_seconds: float


class ScanCancelled(Exception):
    """The caller cancelled a scan after child-process cleanup."""


def clean_environment(home: Path, cache: Path | None = None):
    """No inherited tokens, proxies, PYTHONPATH, Git config, or scanner settings."""
    env = {
        "PATH": "/opt/security/bin:/opt/security/venv/bin:/usr/local/bin:/usr/bin:/bin",
        "HOME": str(home),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "TZ": "UTC",
        "TMPDIR": str(home / ".tmp"),
        "CI": "1",
        "NO_COLOR": "1",
        "TERM": "dumb",
        "PYTHONNOUSERSITE": "1",
        "PYTHONDONTWRITEBYTECODE": "1",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_NO_REPLACE_OBJECTS": "1",
        "SEMGREP_SEND_METRICS": "off",
        "SEMGREP_ENABLE_VERSION_CHECK": "0",
        "DO_NOT_TRACK": "1",
        "SYFT_CHECK_FOR_APP_UPDATE": "false",
        "TRIVY_DISABLE_TELEMETRY": "true",
    }
    if cache is not None:
        env["XDG_CACHE_HOME"] = str(cache)
    return env


def run_process(
    argv,
    cwd: Path,
    timeout,
    *,
    max_output_bytes=16 * 1024 * 1024,
    home=None,
    cache=None,
    file_limit=32 * 1024 * 1024,
):
    """Execute an absolute allowlisted command with bounded pipes and kill its group.

    stdout/stderr are transient parser inputs and MUST NOT be put in reports.
    Killing the group even on normal exit prevents an orphan from holding a pipe.
    """
    if not argv or not Path(argv[0]).is_absolute():
        raise ValueError("scanner executable must be absolute")
    started = time.monotonic()
    if timeout <= 0:
        return ProcessResult("timeout", None, "", "", 0)
    home = Path(home or cwd)
    temporary = home / ".tmp"
    if temporary.is_symlink():
        raise ValueError("scanner temporary storage must not be a symlink")
    temporary.mkdir(mode=0o700, exist_ok=True)

    def limits():
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        resource.setrlimit(resource.RLIMIT_FSIZE, (file_limit, file_limit))
        current = resource.getrlimit(resource.RLIMIT_NOFILE)
        cap = min(current[0], 512) if current[0] >= 0 else 512
        resource.setrlimit(resource.RLIMIT_NOFILE, (cap, current[1]))

    # Defer cancellation until we have a child PID to reap. Raising directly
    # from a signal handler during Popen could orphan a successfully forked child.
    cancelled = []
    previous_handlers = {}

    def cancel(signum, _frame):
        cancelled.append(signum)

    if threading.current_thread() is threading.main_thread():
        for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            previous_handlers[sig] = signal.signal(sig, cancel)
    process = None
    selector = None
    outputs = {"stdout": bytearray(), "stderr": bytearray()}
    status = "complete"
    total = 0
    try:
        try:
            process = subprocess.Popen(
                argv,
                cwd=cwd,
                env=clean_environment(home, cache),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=True,
                preexec_fn=limits,
            )
        except (OSError, subprocess.SubprocessError):
            return ProcessResult(
                "error", None, "", "", round(time.monotonic() - started, 3)
            )
        selector = selectors.DefaultSelector()
        for stream, name in ((process.stdout, "stdout"), (process.stderr, "stderr")):
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ, name)
        while selector.get_map():
            remaining = timeout - (time.monotonic() - started)
            if cancelled:
                break
            if remaining <= 0:
                status = "timeout"
                break
            for key, _ in selector.select(min(0.1, remaining)):
                data = os.read(key.fileobj.fileno(), 65536)
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                room = max(0, max_output_bytes - total)
                outputs[key.data].extend(data[:room])
                total += len(data)
                if total > max_output_bytes:
                    status = "output_limit"
                    break
            if status != "complete":
                break
        while status == "complete" and not cancelled and process.poll() is None:
            remaining = timeout - (time.monotonic() - started)
            if remaining <= 0:
                status = "timeout"
                break
            try:
                process.wait(timeout=min(0.1, remaining))
            except subprocess.TimeoutExpired:
                continue
    finally:
        if process is not None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            process.stdout.close()
            process.stderr.close()
        if selector is not None:
            selector.close()
        for sig, previous in previous_handlers.items():
            signal.signal(sig, previous)
    if cancelled:
        raise ScanCancelled("Security scan was cancelled.")
    return ProcessResult(
        status,
        process.returncode,
        outputs["stdout"].decode("utf-8", "replace"),
        outputs["stderr"].decode("utf-8", "replace"),
        round(time.monotonic() - started, 3),
    )
