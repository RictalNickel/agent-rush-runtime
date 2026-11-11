"""Input validation and bounded, non-executing source snapshots."""

import hashlib
import os
import re
import stat
import time
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import urlsplit

from .reports import text

PROFILES = {
    "quick",
    "standard",
    "secrets",
    "dependencies",
    "sast",
    "lint",
    "supply_chain",
    "iac",
    "malware",
}
IMAGE_RE = re.compile(
    r"[a-z0-9][a-z0-9.-]*(?::[0-9]{1,5})?/(?:[a-z0-9]+(?:[._-][a-z0-9]+)*/)*[a-z0-9]+(?:[._-][a-z0-9]+)*@sha256:[a-f0-9]{64}\Z"
)
PLANNED_EXCLUSIONS = {
    ".git",
    ".security",
    "node_modules",
    ".venv",
    "venv",
    "target",
    "dist",
    ".next",
    "__pycache__",
}
SCANNER_CONTROLS = {
    ".gitleaksignore",
    ".semgrepignore",
    ".trivyignore",
    ".trivyignore.yaml",
}


@dataclass(frozen=True)
class Request:
    action: str
    path: str = "."
    profile: str = "standard"
    offline: bool = True
    timeout_seconds: int = 540
    image: str | None = None
    target: str | None = None

    @classmethod
    def parse(cls, value):
        if not isinstance(value, dict):
            raise ValueError("request must be an object")
        allowed = {
            "doctor": {"timeout_seconds"},
            "scan": {"path", "profile", "offline", "timeout_seconds"},
            "image": {"image", "offline", "timeout_seconds"},
            "web": {"target", "timeout_seconds"},
            "update_databases": {"offline", "timeout_seconds"},
        }
        action = value.get("action")
        if not isinstance(action, str) or action not in allowed:
            raise ValueError("unknown security action")
        # Null Option fields match the native Rust decoder; unknown names never do.
        names = {
            "action",
            "path",
            "profile",
            "offline",
            "timeout_seconds",
            "image",
            "target",
        }
        if set(value) - names:
            raise ValueError("unknown request field")
        args = {k: v for k, v in value.items() if k != "action" and v is not None}
        if set(args) - allowed[action]:
            raise ValueError("field is not valid for this action")
        timeout = args.get("timeout_seconds", 540)
        if type(timeout) is not int or not 10 <= timeout <= 540:
            raise ValueError("timeout_seconds must be an integer between 10 and 540")
        if "offline" in args and type(args["offline"]) is not bool:
            raise ValueError("offline must be a boolean")
        if action == "update_databases" and args.get("offline") is not False:
            raise ValueError("database updates require offline=false")
        if action == "scan":
            path = args.get("path", ".")
            if (
                not isinstance(path, str)
                or not path.strip()
                or len(path.encode()) > 4096
                or "\0" in path
            ):
                raise ValueError("path must be 1–4096 bytes without NUL")
            profile = args.get("profile", "standard")
            if not isinstance(profile, str) or profile not in PROFILES:
                raise ValueError("unknown scan profile")
        if action == "image":
            image = args.get("image")
            if (
                not isinstance(image, str)
                or len(image) > 512
                or not IMAGE_RE.fullmatch(image)
            ):
                raise ValueError(
                    "image must be registry/repository@sha256:<64 lowercase hex digits>"
                )
            registry = image.split("/", 1)[0]
            hostname, _, port_text = registry.partition(":")
            if any(
                not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?", label)
                for label in hostname.split(".")
            ):
                raise ValueError("invalid registry hostname")
            if port_text and not 1 <= int(port_text) <= 65535:
                raise ValueError("invalid registry port")
        if action == "web":
            target = args.get("target")
            if not isinstance(target, str) or len(target) > 200:
                raise ValueError("web target must be a literal loopback HTTP origin")
            try:
                parts = urlsplit(target)
                port = parts.port
            except ValueError as error:
                raise ValueError("invalid web origin") from error
            origin_pattern = r"http://(?:127\.0\.0\.1|\[::1\]):[0-9]{1,5}/?"
            if (
                not re.fullmatch(origin_pattern, target)
                or not port
                or parts.path not in ("", "/")
            ):
                raise ValueError(
                    "web target must be http://127.0.0.1:PORT or http://[::1]:PORT"
                )
        return cls(action=action, **args)


def validate_source(workspace: Path, value: str) -> Path:
    workspace = workspace.resolve(strict=True)
    if ".." in Path(value).parts:
        raise ValueError("parent traversal is not a scan target")
    candidate = Path(value)
    if not candidate.is_absolute():
        candidate = workspace / candidate
    try:
        source = candidate.resolve(strict=True)
        relative = source.relative_to(workspace)
    except (ValueError, OSError) as error:
        raise ValueError(
            "scan target must be an existing directory inside /workspace"
        ) from error
    if ".security" in relative.parts or not source.is_dir():
        raise ValueError(
            "scan target must be a source directory, not security artifacts"
        )
    return source


def secure_directory(path: Path) -> Path:
    """Create an owned private directory, refusing preexisting links/special files."""
    try:
        path.mkdir(mode=0o700)
    except FileExistsError:
        pass
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid():
        raise ValueError("security storage must be an owned directory without symlinks")
    path.chmod(0o700)
    return path


def validate_cache_tree(cache: Path, maximum_entries=20000):
    """Reject planted links/special files before a scanner can update its cache.

    The private workspace remains owned by the same sandbox user; this is a
    defensive preflight, not isolation from a concurrently malicious process.
    """
    seen = 0
    for _base, directories, files, descriptor in os.fwalk(cache, follow_symlinks=False):
        for name in directories + files:
            seen += 1
            if seen > maximum_entries:
                raise ValueError("security cache exceeds its entry limit")
            info = os.stat(name, dir_fd=descriptor, follow_symlinks=False)
            if info.st_uid != os.geteuid() or not (
                stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode)
            ):
                raise ValueError(
                    "security cache must contain owned regular files and directories without links"
                )
            if stat.S_ISREG(info.st_mode) and info.st_nlink != 1:
                raise ValueError("security cache files must not have hard links")


def snapshot_source(
    source: Path,
    destination: Path,
    *,
    max_file_bytes=25 * 1024 * 1024,
    max_total_bytes=256 * 1024 * 1024,
    max_files=20000,
    include_dependencies=False,
    deadline=None,
):
    """Copy regular files via directory descriptors; never follow source links.

    Snapshot copying is deliberately separate from scanner execution. No package
    manager, build system, repository plugin, Git filter or hook is invoked.
    """
    destination.mkdir(mode=0o700)
    result = {
        "complete": True,
        "files": 0,
        "bytes": 0,
        "skipped": [],
        "excluded_directories": [],
        "git_history": "not_scanned",
    }
    digest = hashlib.sha256()
    exclusions = PLANNED_EXCLUSIONS - (
        {"node_modules", ".venv", "venv"} if include_dependencies else set()
    )

    def skipped(path, reason, incomplete=True):
        if len(result["skipped"]) < 2000:
            result["skipped"].append({"path": text(str(path), 512), "reason": reason})
        else:
            result["skipped_records_truncated"] = True
        if incomplete:
            result["complete"] = False

    def walk_error(_error):
        skipped(".", "unreadable_directory")

    for base, directories, filenames, directory_fd in os.fwalk(
        source, follow_symlinks=False, onerror=walk_error
    ):
        relative = Path(base).relative_to(source)
        if deadline is not None and time.monotonic() >= deadline:
            skipped(relative, "snapshot_timeout")
            break
        kept = []
        for name in sorted(directories):
            item = relative / name
            try:
                info = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                if stat.S_ISLNK(info.st_mode):
                    skipped(item, "symlink")
                elif name in exclusions:
                    if len(result["excluded_directories"]) < 200:
                        result["excluded_directories"].append(text(str(item), 512))
                else:
                    kept.append(name)
            except OSError:
                skipped(item, "unreadable")
        directories[:] = kept
        (destination / relative).mkdir(mode=0o700, parents=True, exist_ok=True)
        for name in sorted(filenames):
            item = relative / name
            if name in SCANNER_CONTROLS:
                skipped(item, "untrusted_scanner_control", incomplete=False)
                continue
            if name == ".git":
                skipped(item, "git_worktree_metadata", incomplete=False)
                continue
            if result["files"] >= max_files or (
                deadline is not None and time.monotonic() >= deadline
            ):
                skipped(item, "snapshot_limit")
                directories[:] = []
                break
            descriptor = None
            try:
                before = os.stat(name, dir_fd=directory_fd, follow_symlinks=False)
                if stat.S_ISLNK(before.st_mode):
                    skipped(item, "symlink")
                    continue
                if not stat.S_ISREG(before.st_mode):
                    skipped(item, "special_file")
                    continue
                if before.st_size > max_file_bytes:
                    skipped(item, "file_size_limit")
                    continue
                if result["bytes"] + before.st_size > max_total_bytes:
                    skipped(item, "total_size_limit")
                    continue
                descriptor = os.open(
                    name,
                    os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                    dir_fd=directory_fd,
                )
                info = os.fstat(descriptor)
                if not stat.S_ISREG(info.st_mode) or (info.st_dev, info.st_ino) != (
                    before.st_dev,
                    before.st_ino,
                ):
                    skipped(item, "source_changed")
                    continue
                with os.fdopen(descriptor, "rb") as stream:
                    descriptor = None
                    data = stream.read(max_file_bytes + 1)
                    after = os.fstat(stream.fileno())
                if (
                    len(data) > max_file_bytes
                    or result["bytes"] + len(data) > max_total_bytes
                ):
                    skipped(item, "file_size_limit")
                    continue
                if (info.st_size, info.st_mtime_ns) != (
                    after.st_size,
                    after.st_mtime_ns,
                ):
                    skipped(item, "source_changed")
                    continue
                output = destination / item
                with output.open("xb") as stream:
                    stream.write(data)
                output.chmod(
                    0o400
                )  # No executable source, no scanner writes to source.
                digest.update(str(item).encode("utf-8", "surrogateescape") + b"\0")
                digest.update(hashlib.sha256(data).digest())
                result["files"] += 1
                result["bytes"] += len(data)
            except (OSError, UnicodeError):
                skipped(item, "unreadable")
            finally:
                if descriptor is not None:
                    os.close(descriptor)
    result["sha256"] = digest.hexdigest()
    return result
