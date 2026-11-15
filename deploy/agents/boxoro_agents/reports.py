"""Private reports with bounded native-artifact manifests."""

import hashlib
import json
import os
import stat
import time
from datetime import datetime, timezone
from pathlib import Path

from .process import MAX_ARTIFACT_BYTES


def utc_now():
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def open_directory(path):
    """Open an absolute directory without following any path component link."""
    path = Path(path).absolute()
    if ".." in path.parts:
        raise ValueError("report directory cannot contain parent traversal")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK
    descriptor = os.open(path.anchor, flags)
    try:
        for component in path.parts[1:]:
            child = os.open(component, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def private_write(path, data):
    if isinstance(data, str):
        data = data.encode("utf-8")
    if len(data) > MAX_ARTIFACT_BYTES:
        raise ValueError("report exceeds the 25 MiB artifact limit")
    parent = open_directory(path.parent)
    try:
        descriptor = os.open(path.name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=parent)
    finally:
        os.close(parent)
    with os.fdopen(descriptor, "wb") as stream:
        stream.write(data)


def artifact_inventory(root, workspace, deadline):
    """Expose bounded owned files through no-follow directory descriptors.

    os.walk(followlinks=False) still follows a replaced root directory, and a
    path-based lstat/open pair can race on intermediate directories. Keep every
    lookup relative to the opened parent so neither case reads outside this tree.
    """
    artifacts = []
    complete = True
    total = 0
    entries = 0
    directory_flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_NONBLOCK

    def identity(info):
        return info.st_dev, info.st_ino

    def walk(parent, relative, depth):
        nonlocal complete, total, entries
        # Limit names while iterating; os.listdir could allocate an unbounded
        # directory listing before the entry quota is applied.
        with os.scandir(parent) as iterator:
            names = []
            for item in iterator:
                entries += 1
                if entries > 1024 or time.monotonic() >= deadline:
                    complete = False
                    return
                names.append(item.name)
        for name in sorted(names):
            descriptor = None
            try:
                if time.monotonic() >= deadline:
                    complete = False
                    return
                info = os.stat(name, dir_fd=parent, follow_symlinks=False)
                if info.st_uid != os.geteuid():
                    complete = False
                    continue
                if stat.S_ISDIR(info.st_mode):
                    if depth >= 64:
                        complete = False
                        continue
                    descriptor = os.open(name, directory_flags, dir_fd=parent)
                    if identity(os.fstat(descriptor)) != identity(info):
                        complete = False
                        continue
                    walk(descriptor, relative / name, depth + 1)
                    if identity(os.stat(name, dir_fd=parent, follow_symlinks=False)) != identity(info):
                        complete = False
                    continue
                if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > MAX_ARTIFACT_BYTES or total + info.st_size > 100 * 1024 * 1024:
                    complete = False
                    continue
                descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
                opened = os.fstat(descriptor)
                if identity(opened) != identity(info):
                    complete = False
                    continue
                digest = hashlib.sha256()
                count = 0
                with os.fdopen(descriptor, "rb") as stream:
                    descriptor = None
                    while chunk := stream.read(1024 * 1024):
                        count += len(chunk)
                        if count > MAX_ARTIFACT_BYTES or time.monotonic() >= deadline:
                            break
                        digest.update(chunk)
                    after = os.fstat(stream.fileno())
                if count != info.st_size or count > MAX_ARTIFACT_BYTES or time.monotonic() >= deadline or (after.st_size, after.st_mtime_ns) != (info.st_size, info.st_mtime_ns):
                    complete = False
                    continue
                total += count
                path = root / relative / name
                artifacts.append({"path": str(path.relative_to(workspace)), "bytes": count, "sha256": digest.hexdigest()})
            except OSError:
                complete = False
            finally:
                if descriptor is not None:
                    os.close(descriptor)
    descriptor = None
    try:
        descriptor = open_directory(root)
        original = os.fstat(descriptor)
        if original.st_uid != os.geteuid():
            return [], False
        walk(descriptor, Path(), 0)
        if identity(root.lstat()) != identity(original):
            return [], False
    except OSError:
        return [], False
    finally:
        if descriptor is not None:
            os.close(descriptor)
    return artifacts, complete


def report_gate(checks):
    if any(check["status"] not in {"passed", "findings"} for check in checks):
        return "incomplete"
    if any(check["status"] == "findings" for check in checks):
        return "fail"
    return "pass"


def write_reports(result, output, workspace):
    result["reports"] = {
        "json": str((output / "result.json").relative_to(workspace)),
        "markdown": str((output / "report.md").relative_to(workspace)),
    }
    private_write(output / "result.json", json.dumps(result, indent=2, ensure_ascii=True, allow_nan=False) + "\n")
    lines = [
        "# Boxoro specialist report",
        "",
        "- Run: " + result["id"],
        "- Action: " + result["action"],
        "- Specialist: " + (result.get("agent") or "all"),
        "- Scope: " + result.get("scope", {}).get("kind", "agent") + " / " + result.get("scope", {}).get("id", result.get("agent") or "all"),
        "- Gate: **" + result["gate"] + "**",
        "- Started: " + result["started_at"],
        "- Completed: " + result["completed_at"],
        "- Catalog SHA-256: `" + result["catalog_sha256"] + "`",
        "",
        "| Operation | Tool | Status | Exit | Seconds |",
        "| --- | --- | --- | --- | --- |",
    ]
    for check in result["checks"]:
        lines.append("| " + " | ".join(str(check.get(key, "—")) for key in ("operation", "tool", "status", "exit_code", "duration_seconds")) + " |")
    lines += ["", "## Coverage and limits", ""]
    lines += ["- " + note for note in result["limitations"]]
    for check in result["checks"]:
        if check.get("reason"):
            lines += ["", "### " + check.get("operation", check["tool"]), "", check["reason"]]
        notes = check.get("notes", [])
        if notes:
            lines += ["", notes if isinstance(notes, str) else " ".join(notes)]
        if check.get("sources"):
            lines += ["", "Sources: " + ", ".join(check["sources"])]
    lines += ["", "Native logs and reports can contain project data or secrets. Review before sharing.", ""]
    private_write(output / "report.md", "\n".join(lines))
