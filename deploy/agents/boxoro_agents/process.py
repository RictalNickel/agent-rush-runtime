"""Bounded command execution inside the existing Boxoro worker sandbox."""

import os
import resource
import selectors
import signal
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path

from .policy import secure_directory

MAX_STREAM_BYTES = 8 * 1024 * 1024
MAX_ARTIFACT_BYTES = 25 * 1024 * 1024
MAX_CHILD_FILE_BYTES = 256 * 1024 * 1024


@dataclass
class ProcessResult:
    status: str
    returncode: int | None
    stdout: bytes
    stderr: bytes
    duration_seconds: float


def clean_environment(home, workspace=None):
    home = Path(home)
    environment = {
        "PATH": "/opt/box/node/bin:/opt/box/agents/bin:/usr/local/bin:/usr/bin:/bin",
        "HOME": str(home),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "TZ": "UTC",
        "TMPDIR": str(home / "tmp"),
        "XDG_CACHE_HOME": str(home / "cache"),
        "XDG_CONFIG_HOME": str(home / "config"),
        "CI": "1",
        "NO_COLOR": "1",
        "TERM": "dumb",
        "PYTHONNOUSERSITE": "1",
        "PYTHONSAFEPATH": "1",
        "PYTHONDONTWRITEBYTECODE": "1",
        "PYTEST_DISABLE_PLUGIN_AUTOLOAD": "1",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": "/dev/null",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_NO_REPLACE_OBJECTS": "1",
        "DO_NOT_TRACK": "1",
        "SEMGREP_SEND_METRICS": "off",
        "SEMGREP_ENABLE_VERSION_CHECK": "0",
        "DBT_SEND_ANONYMOUS_USAGE_STATS": "false",
        "NEXT_TELEMETRY_DISABLED": "1",
        "HUSKY": "0",
        "NPM_CONFIG_UPDATE_NOTIFIER": "false",
        "PLAYWRIGHT_BROWSERS_PATH": "/ms-playwright",
        "CHROME_PATH": "/opt/box/chromium",
        "PUPPETEER_EXECUTABLE_PATH": "/opt/box/chromium",
        "HTTPIE_CONFIG_DIR": "/opt/box/agents/boxoro_agents/httpie-config",
    }
    if workspace is not None:
        environment["BOXORO_WORKSPACE"] = str(workspace)
    return environment


def run_process(argv, cwd, timeout, *, home=None, workspace=None, max_output_bytes=MAX_STREAM_BYTES):
    """Use argv without a shell; always reap the process group, including on TERM.

    Output caps apply independently to each byte stream. File-size limits are
    per file, not a disk quota; the worker's volume is the total storage boundary.
    These controls do not sandbox project code or impose network restrictions.
    """
    if not argv or not Path(argv[0]).is_absolute() or any(not isinstance(item, str) or "\0" in item for item in argv):
        raise ValueError("specialist executable must be an absolute registered path")
    started = time.monotonic()
    if timeout <= 0:
        return ProcessResult("timeout", None, b"", b"", 0)
    home = secure_directory(Path(home or cwd))
    for name in ("tmp", "cache", "config"):
        secure_directory(home / name)

    def limits():
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        # Builds and local test databases may exceed the downloadable-report cap.
        # This is per file; the worker volume remains the total disk quota.
        resource.setrlimit(resource.RLIMIT_FSIZE, (MAX_CHILD_FILE_BYTES, MAX_CHILD_FILE_BYTES))
        soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
        cap = min(soft, 1024) if soft >= 0 else 1024
        resource.setrlimit(resource.RLIMIT_NOFILE, (cap, hard))

    cancelled = []
    previous = {}

    def cancel(signum, _frame):
        # Do not raise during Popen: a successfully forked child might be lost.
        cancelled.append(signum)

    if threading.current_thread() is threading.main_thread():
        for sig in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            previous[sig] = signal.signal(sig, cancel)
    process = None
    selector = None
    output = {"stdout": bytearray(), "stderr": bytearray()}
    status = "complete"
    try:
        try:
            process = subprocess.Popen(
                argv,
                cwd=cwd,
                env=clean_environment(home, workspace),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=True,
                preexec_fn=limits,
            )
        except (OSError, subprocess.SubprocessError):
            return ProcessResult("error", None, b"", b"", round(time.monotonic() - started, 3))
        selector = selectors.DefaultSelector()
        for stream, name in ((process.stdout, "stdout"), (process.stderr, "stderr")):
            os.set_blocking(stream.fileno(), False)
            selector.register(stream, selectors.EVENT_READ, name)
        while selector.get_map():
            remaining = timeout - (time.monotonic() - started)
            if cancelled:
                status = "cancelled"
                break
            if remaining <= 0:
                status = "timeout"
                break
            # An exited parent cannot leave descendants holding its pipes open.
            if process.poll() is not None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for key, _ in selector.select(min(0.1, remaining)):
                data = os.read(key.fileobj.fileno(), 65536)
                if not data:
                    selector.unregister(key.fileobj)
                    continue
                room = max(0, max_output_bytes - len(output[key.data]))
                output[key.data].extend(data[:room])
                if len(data) > room:
                    status = "output_limit"
                    break
            if status != "complete":
                break
        while status == "complete" and process.poll() is None:
            remaining = timeout - (time.monotonic() - started)
            if cancelled:
                status = "cancelled"
                break
            if remaining <= 0:
                status = "timeout"
                break
            try:
                process.wait(timeout=min(0.1, remaining))
            except subprocess.TimeoutExpired:
                pass
        if cancelled:
            status = "cancelled"
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
        for sig, handler in previous.items():
            signal.signal(sig, handler)
    return ProcessResult(status, process.returncode, bytes(output["stdout"]), bytes(output["stderr"]), round(time.monotonic() - started, 3))
