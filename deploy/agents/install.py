#!/usr/bin/env python3
"""Install the reviewed specialist tool lock into an image or scratch tool prefix.

This is a build-time program, never a workspace-agent operation. Python tools
have separate virtual environments and npm tools have separate locked prefixes.
Upstream npm lifecycle scripts run only when explicitly declared in the lock.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import gzip
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request
import zipfile
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

MAX_ARTIFACT = 512 * 1024 * 1024
ALLOWED_HOSTS = {
    "github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com",
    "raw.githubusercontent.com", "nodejs.org", "get.helm.sh", "dl.k8s.io",
    "cdn.dl.k8s.io", "releases.hashicorp.com", "get.opentofu.org",
}
ID = re.compile(r"[a-z0-9][a-z0-9-]{0,63}\Z")


class InstallError(Exception):
    """A lock or installation prerequisite is invalid."""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for data in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(data)
    return digest.hexdigest()


def locked_file(root: Path, relative: str, digest: str) -> Path:
    parts = PurePosixPath(relative)
    if parts.is_absolute() or ".." in parts.parts or "\\" in relative:
        raise InstallError("lock path must stay within the installation bundle")
    path = root / relative
    cursor = root
    for part in parts.parts:
        cursor /= part
        if cursor.is_symlink():
            raise InstallError("lock files cannot traverse symlinks")
    if not path.is_file() or not re.fullmatch(r"[a-f0-9]{64}", digest):
        raise InstallError("missing lock file or invalid digest")
    if sha256(path) != digest:
        raise InstallError("lock file digest mismatch: " + relative)
    return path


def select_tools(catalog: dict, family: str, agent: str | None, optional: bool) -> list[str]:
    if agent is not None:
        if agent not in catalog["agents"]:
            raise InstallError("unknown agent")
        selected = catalog["agents"][agent]["tools"]
    elif family == "suite":
        selected = catalog["tools"]
    elif family == "core":
        selected = {key for key, tool in catalog["tools"].items() if tool.get("family") == "core"}
    else:
        members = [item for item in catalog["agents"].values() if item["family"] == family]
        if not members:
            raise InstallError("unknown image family")
        selected = {tool for member in members for tool in member["tools"]}
    levels = {"image", "optional"} if optional else {"image"}
    return sorted({key for key in selected if catalog["tools"][key]["availability"] in levels})


def validate_url(value: str) -> None:
    parsed = urllib.parse.urlsplit(value)
    if (parsed.scheme != "https" or parsed.hostname not in ALLOWED_HOSTS
            or parsed.username or parsed.password or parsed.port not in (None, 443)
            or parsed.fragment):
        raise InstallError("unapproved release URL")


class Redirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        validate_url(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def artifact(item: dict, cache: Path, offline: bool) -> Path:
    digest = item["sha256"]
    if not re.fullmatch(r"[0-9a-f]{64}", digest):
        raise InstallError("invalid artifact digest")
    validate_url(item["url"])
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / digest
    if path.is_symlink():
        raise InstallError("artifact cache cannot contain symlinks")
    if not path.exists():
        if offline:
            raise InstallError("missing cached binary in offline installation")
        fd, temporary = tempfile.mkstemp(prefix="download-", dir=cache)
        try:
            opener = urllib.request.build_opener(Redirects())
            request = urllib.request.Request(item["url"], headers={"User-Agent": "Boxoro-image-build/1"})
            with os.fdopen(fd, "wb") as output, opener.open(request, timeout=90) as response:
                validate_url(response.geturl())
                size = 0
                while chunk := response.read(1024 * 1024):
                    size += len(chunk)
                    if size > MAX_ARTIFACT:
                        raise InstallError("artifact exceeds download ceiling")
                    output.write(chunk)
            temporary_path = Path(temporary)
            if sha256(temporary_path) != digest or temporary_path.stat().st_size != item["size"]:
                raise InstallError("download digest or size mismatch")
            os.replace(temporary_path, path)
        finally:
            Path(temporary).unlink(missing_ok=True)
    if not path.is_file() or path.stat().st_size != item["size"] or sha256(path) != digest:
        raise InstallError("cached binary digest or size mismatch")
    return path


def safe_member(name: str) -> None:
    value = PurePosixPath(name)
    if value.is_absolute() or ".." in value.parts or "\\" in name or "\x00" in name:
        raise InstallError("unsafe release archive member")


def write_stream(source, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix="install-", dir=destination.parent)
    try:
        with os.fdopen(fd, "wb") as target:
            size = 0
            while data := source.read(1024 * 1024):
                size += len(data)
                if size > MAX_ARTIFACT:
                    raise InstallError("expanded binary exceeds size ceiling")
                target.write(data)
        os.chmod(temporary, 0o755)
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)


def extract_binary(source: Path, kind: str, member: str | None, destination: Path) -> None:
    if kind == "raw":
        with source.open("rb") as stream:
            write_stream(stream, destination)
    elif kind == "gz":
        with gzip.open(source, "rb") as stream:
            write_stream(stream, destination)
    elif kind == "tar":
        with tarfile.open(source, "r:*") as archive:
            selected = []
            count = 0
            for item in archive:
                count += 1
                if count > 10000:
                    raise InstallError("archive member count exceeds ceiling")
                safe_member(item.name)
                if not (item.isfile() or item.isdir()) or item.size > MAX_ARTIFACT:
                    raise InstallError("release archive contains a link or oversized member")
                if item.name == member:
                    selected.append(item)
            if len(selected) != 1 or not selected[0].isfile():
                raise InstallError("archive must have one exact executable member")
            with archive.extractfile(selected[0]) as stream:
                write_stream(stream, destination)
    elif kind == "zip":
        with zipfile.ZipFile(source) as archive:
            members = archive.infolist()
            if len(members) > 10000:
                raise InstallError("archive member count exceeds ceiling")
            for item in members:
                safe_member(item.filename)
                mode = (item.external_attr >> 16) & 0o170000
                if mode not in (0, 0o100000, 0o040000) or item.file_size > MAX_ARTIFACT:
                    raise InstallError("release zip contains a link or oversized member")
            selected = [item for item in members if item.filename == member and not item.is_dir()]
            if len(selected) != 1:
                raise InstallError("zip must have one exact executable member")
            with archive.open(selected[0]) as stream:
                write_stream(stream, destination)
    else:
        raise InstallError("unsupported release artifact format")


def call(argv: list[str], **kwargs) -> None:
    subprocess.run(argv, check=True, timeout=1800, **kwargs)


def symlink(destination: Path, source: Path) -> None:
    if not source.is_file() or not os.access(source, os.X_OK):
        raise InstallError("expected executable was not installed: " + str(source))
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() or destination.is_symlink():
        destination.unlink()
    destination.symlink_to(source.resolve())


def install_tool(key: str, item: dict, root: Path, prefix: Path, cache: Path,
                 offline: bool, python: str, npm: str) -> None:
    if not ID.fullmatch(key):
        raise InstallError("invalid locked tool ID")
    destination = prefix / "bin" / key
    kind = item["kind"]
    if kind == "python":
        requirements = locked_file(root, item["requirements"], item["requirements_sha256"])
        environment = prefix / "python" / key
        # pip does not recheck installed files against --require-hashes and a
        # reused venv can retain dependencies removed from the reviewed lock.
        call([python, "-m", "venv", "--clear", str(environment)])
        argv = [str(environment / "bin/python"), "-m", "pip", "--isolated", "install",
                "--disable-pip-version-check", "--require-hashes"]
        wheel_cache = ["--cache-dir", str(cache / "pip")]
        source_build = item.get("allow_source", False)
        if source_build:
            build_requirements = locked_file(root, item["build_requirements"], item["build_requirements_sha256"])
            build_argv = argv + wheel_cache + ["--only-binary=:all:", "-r", str(build_requirements)]
            if offline:
                build_argv.extend(["--no-index", "--find-links", str(cache / "wheels")])
            call(build_argv)
            # Only downloaded wheels are reused. Locally built wheels are not
            # covered by the lock's artifact hashes, so rebuild source opt-ins.
            argv.extend(["--no-build-isolation", "--no-cache-dir"])
        else:
            argv.extend(wheel_cache + ["--only-binary=:all:"])
        argv.extend(["-r", str(requirements)])
        if offline:
            argv.extend(["--no-index", "--find-links", str(cache / "wheels")])
        call(argv)
        call([str(environment / "bin/python"), "-m", "pip", "check"])
        if item.get("library"):
            distribution = item["package"].split("[")[0]
            code = "import importlib.metadata; print(importlib.metadata.version(" + repr(distribution) + "))"
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text("#!/bin/sh\nexec " + shlex.quote(str(environment / "bin/python"))
                                   + " -I -c " + shlex.quote(code) + "\n")
            destination.chmod(0o755)
        else:
            symlink(destination, environment / "bin" / item["executable"])
    elif kind == "npm":
        environment = prefix / "node" / key
        environment.mkdir(parents=True, exist_ok=True)
        for field, name in (("package_json", "package.json"), ("package_lock", "package-lock.json")):
            source = locked_file(root, item[field], item[field + "_sha256"])
            shutil.copyfile(source, environment / name)
        argv = [npm, "ci", "--no-audit", "--no-fund", "--omit=dev", "--registry=https://registry.npmjs.org/"]
        if not item.get("lifecycle_scripts", False):
            argv.append("--ignore-scripts")
        else:
            argv.append("--strict-allow-scripts")
        if offline:
            argv.append("--offline")
        env = dict(os.environ, NPM_CONFIG_CACHE=str(cache / "npm"),
                   NPM_CONFIG_USERCONFIG="/dev/null", PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD="1",
                   PUPPETEER_SKIP_DOWNLOAD="true")
        call(argv, cwd=environment, env=env)
        if item.get("library"):
            package = environment / "node_modules" / item["package"] / "package.json"
            code = "console.log(JSON.parse(require('node:fs').readFileSync(" + json.dumps(str(package)) + ",'utf8')).version)"
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text("#!/bin/sh\nexec node -e " + shlex.quote(code) + "\n")
            destination.chmod(0o755)
        else:
            symlink(destination, environment / "node_modules/.bin" / item["executable"])
    elif kind == "binary":
        source = artifact(item, cache / "binaries", offline)
        extract_binary(source, item["archive"], item.get("member"), destination)
    elif kind in ("apt", "builtin"):
        source = item["path"]
        if not isinstance(source, str) or not source.startswith(("/usr/bin/", "/bin/", "/opt/box/bin/", "/opt/rust/")):
            raise InstallError("system executable is outside trusted image locations")
        if kind == "apt":
            # Debian's pg_wrapper dispatches using the canonical argv[0]. A
            # resolved symlink named pg-restore would lose that information.
            original = Path(source)
            if not original.is_file() or not os.access(original, os.X_OK):
                raise InstallError("system executable is not installed: " + source)
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.unlink(missing_ok=True)
            destination.write_text("#!/bin/sh\nexec " + shlex.quote(source) + ' "$@"\n')
            destination.chmod(0o755)
        else:
            symlink(destination, Path(source))
    else:
        raise InstallError("tool has no image installation method: " + key)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--family", default="suite")
    parser.add_argument("--agent")
    parser.add_argument("--tools", help="comma-separated explicit subset for build qualification")
    parser.add_argument("--jobs", type=int, default=1, help="independent package installations in parallel, 1..6")
    parser.add_argument("--include-optional", action="store_true")
    parser.add_argument("--prefix", type=Path, default=Path("/opt/box/agents"))
    parser.add_argument("--cache", type=Path, default=Path("/tmp/boxoro-agent-downloads"))
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--python", default=sys.executable)
    parser.add_argument("--npm", default=shutil.which("npm") or "/usr/bin/npm")
    parser.add_argument("--list-packages", action="store_true", help="print required apt packages only")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    catalog = json.loads((root / "catalog.json").read_text())
    lock = json.loads((root / "install.lock.json").read_text())
    selected = select_tools(catalog, args.family, args.agent, args.include_optional)
    if args.tools:
        requested = set(args.tools.split(","))
        if not requested or not requested.issubset(selected):
            raise InstallError("explicit tools must belong to the selected installable set")
        selected = sorted(requested)
    if any(key not in lock["tools"] for key in selected):
        raise InstallError("selected tool has no resolved installation lock")
    if args.list_packages:
        print(" ".join(sorted({lock["tools"][key]["package"] for key in selected
                              if lock["tools"][key]["kind"] == "apt"})))
        return 0
    args.prefix.mkdir(parents=True, exist_ok=True)
    manifest_path = args.prefix / "installed.json"
    previous = json.loads(manifest_path.read_text()) if manifest_path.exists() else {"tools": {}}
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, min(args.jobs, 6))) as executor:
        futures = {}
        for key in selected:
            item = lock["tools"][key]
            print("Installing " + key + " (" + item["kind"] + ")", flush=True)
            future = executor.submit(install_tool, key, item, root, args.prefix, args.cache,
                                     args.offline, args.python, args.npm)
            futures[future] = key
        for future in concurrent.futures.as_completed(futures):
            key = futures[future]
            future.result()
            item = lock["tools"][key]
            previous["tools"][key] = {"version": item.get("version"), "kind": item["kind"],
                                       "library": item.get("library", False),
                                       "executable": str(args.prefix / "bin" / key)}
    previous.update(schema_version=1, installed_at=datetime.now(timezone.utc).isoformat(),
                    lock_sha256=sha256(root / "install.lock.json"), family=args.family)
    manifest_path.write_text(json.dumps(previous, indent=2, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (InstallError, OSError, subprocess.SubprocessError, ValueError, KeyError) as error:
        print("Agent tool installation failed: " + str(error), file=sys.stderr)
        raise SystemExit(1)
