#!/usr/bin/env python3
"""Maintainer-only resolution of catalog packages into reviewed, committed locks.

Normal image builds consume install.lock.json and never resolve latest versions.
Run this explicitly to refresh dependencies, review the diff, then qualify images.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent

# Install-script policy for the pinned npm distribution. Unknown new scripts in
# these environments require maintainer review, even if the parent tool is trusted.
NPM_SCRIPTS = {
    "buf": {"@bufbuild/buf": True},
    "vitest": {"fsevents": False},
    "promptfoo": {"esbuild": True, "fsevents": False},
    "odiff": {}, "sharp-cli": {}, "appium": {"appium": True},
}


def script_approvals(key: str, lock: dict) -> dict:
    approved = {}
    for path, package in lock["packages"].items():
        if not path or not package.get("hasInstallScript"):
            continue
        name = path.rsplit("node_modules/", 1)[-1]
        if name not in NPM_SCRIPTS.get(key, {}):
            raise RuntimeError("unreviewed npm install script: " + key + ": " + name)
        approved[name + "@" + package["version"]] = NPM_SCRIPTS[key][name]
    return approved


def metadata(url: str):
    request = urllib.request.Request(url, headers={"User-Agent": "Boxoro-toolchain-lock/1"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def verify_asset(path: Path, asset: dict) -> str:
    if path.is_symlink() or not path.is_file():
        raise RuntimeError("release cache entry must be a regular file")
    size = path.stat().st_size
    if size > 512 * 1024 * 1024:
        raise RuntimeError("binary artifact exceeds 512MiB ceiling")
    if asset.get("size") is not None and size != asset["size"]:
        raise RuntimeError("release API size disagrees with downloaded binary")
    actual = digest(path)
    expected = asset.get("digest", "")
    if expected and expected != "sha256:" + actual:
        raise RuntimeError("release API digest disagrees with downloaded binary")
    return actual


def locked_file(item: dict, key: str, path: Path) -> None:
    item[key] = str(path.relative_to(ROOT))
    item[key + "_sha256"] = digest(path)


def command(argv: list[str], *, cwd: Path | None = None, timeout: int = 600) -> None:
    result = subprocess.run(argv, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(result.stderr[-4000:] or result.stdout[-4000:])


def resolve_python(key: str, tool: dict) -> dict:
    install = tool["install"]
    package, version = install["package"], install.get("version", "")
    if version and not version.startswith(("=", ">", "<", "!", "~")):
        version = "==" + version
    directory = ROOT / "locks/python"
    directory.mkdir(parents=True, exist_ok=True)
    source, output = directory / (key + ".in"), directory / (key + ".lock")
    source.write_text(package + version + "\n")
    argv = ["uv", "pip", "compile", str(source), "--output-file", str(output),
             "--python-version", "3.12", "--python-platform", "x86_64-manylinux_2_34",
             "--generate-hashes", "--no-header", "--no-annotate", "--upgrade", "--quiet"]
    if not install.get("allow_source", False):
        argv.extend(["--only-binary", ":all:"])
    command(argv)
    normalized = re.sub(r"[-_.]+", "-", package.split("[")[0]).lower()
    resolved = None
    for line in output.read_text().splitlines():
        match = re.match(r"([A-Za-z0-9_.-]+)==([^ ;\\]+)", line)
        if match and re.sub(r"[-_.]+", "-", match[1]).lower() == normalized:
            resolved = match[2]
            break
    if not resolved:
        raise RuntimeError("resolved lock omitted the requested package")
    item = {"kind": "python", "package": package, "version": resolved,
            "executable": install.get("executable", ""), "library": not bool(install.get("executable"))}
    locked_file(item, "requirements", output)
    if install.get("allow_source", False):
        # This opt-in is limited to reviewed build-time packages. Build isolation
        # would otherwise download unpinned build requirements outside the lock.
        build_input = directory / (key + "-build.in")
        build_output = directory / (key + "-build.lock")
        build_input.write_text("setuptools\nwheel\n")
        command(["uv", "pip", "compile", str(build_input), "--output-file", str(build_output),
                 "--python-version", "3.12", "--python-platform", "x86_64-manylinux_2_34",
                 "--generate-hashes", "--no-header", "--no-annotate", "--upgrade",
                 "--only-binary", ":all:", "--quiet"])
        item["allow_source"] = True
        locked_file(item, "build_requirements", build_output)
    return item


def resolve_npm(key: str, tool: dict) -> dict:
    install = tool["install"]
    version = install.get("version", "").replace(",", " ").lstrip("=") or "*"
    directory = ROOT / "locks/npm" / key
    directory.mkdir(parents=True, exist_ok=True)
    package_json = directory / "package.json"
    value = {"name": "boxoro-tool-" + key, "version": "1.0.0", "private": True,
             "dependencies": {install["package"]: version}}
    if install.get("overrides"):
        value["overrides"] = install["overrides"]
    package_lock = directory / "package-lock.json"
    # Resolve in a clean directory: npm otherwise retains an old lock satisfying
    # '*' even during an explicit maintainer --refresh. Keep the reviewed files
    # intact if the network, resolution or executable validation fails.
    with tempfile.TemporaryDirectory(prefix="resolve-", dir=directory) as temporary:
        stage = Path(temporary)
        (stage / "package.json").write_text(json.dumps(value, indent=2) + "\n")
        command(["npm", "install", "--package-lock-only", "--ignore-scripts", "--no-audit", "--no-fund",
                 "--registry=https://registry.npmjs.org/"], cwd=stage)
        lock = json.loads((stage / "package-lock.json").read_text())
    top = lock["packages"]["node_modules/" + install["package"]]
    resolved = top["version"]
    value["dependencies"][install["package"]] = resolved
    lock["packages"][""]["dependencies"][install["package"]] = resolved
    if key in NPM_SCRIPTS:
        value["allowScripts"] = script_approvals(key, lock)
    executable = install.get("executable", "")
    bins = top.get("bin", {})
    if executable and isinstance(bins, dict) and executable not in bins:
        raise RuntimeError(f"package does not expose expected executable {executable}; available: {list(bins)}")
    package_json.write_text(json.dumps(value, indent=2) + "\n")
    package_lock.write_text(json.dumps(lock, indent=2) + "\n")
    item = {"kind": "npm", "package": install["package"], "version": resolved,
            "executable": executable, "library": not bool(executable),
            # Native/transpiler installers are confined to trusted image builds.
            "lifecycle_scripts": key in NPM_SCRIPTS}
    locked_file(item, "package_json", package_json)
    locked_file(item, "package_lock", package_lock)
    return item


def choose_asset(key: str, assets: list[dict]) -> dict:
    patterns = {
        "syft": r"_linux_amd64\.tar\.gz$", "grype": r"_linux_amd64\.tar\.gz$",
        "trivy": r"_Linux-64bit\.tar\.gz$", "gitleaks": r"_linux_x64\.tar\.gz$",
        "actionlint": r"_linux_amd64\.tar\.gz$", "hadolint": r"^hadolint-[Ll]inux-x86_64$",
        "opentofu": r"_linux_amd64\.zip$", "tflint": r"^tflint_linux_amd64\.zip$",
        "conftest": r"_Linux_x86_64\.tar\.gz$", "kubeconform": r"linux-amd64\.tar\.gz$",
        "kustomize": r"_linux_amd64\.tar\.gz$", "k6": r"-linux-amd64\.tar\.gz$",
        "oasdiff": r"_linux_amd64\.tar\.gz$", "grpcurl": r"_linux_x86_64\.tar\.gz$",
        "vale": r"_Linux_64-bit\.tar\.gz$", "lychee": r"x86_64-unknown-linux-musl\.tar\.gz$",
        "vegeta": r"_linux_amd64\.tar\.gz$", "cosign": r"^cosign-linux-amd64$",
        "dbmate": r"^dbmate-linux-amd64$", "trufflehog": r"_linux_amd64\.tar\.gz$",
    }
    pattern = patterns.get(key)
    matches = [asset for asset in assets if pattern and re.search(pattern, asset["name"])]
    if len(matches) != 1:
        raise RuntimeError(f"no unique reviewed linux/amd64 artifact for {key}: {[a['name'] for a in assets]}")
    return matches[0]


def resolve_binary(key: str, tool: dict, cache: Path) -> dict:
    install = tool["install"]
    repository = install["package"]
    version = install.get("version", "")
    url = f"https://api.github.com/repos/{repository}/releases/" + ("tags/" + version if version else "latest")
    release = metadata(url)
    if release.get("draft") or release.get("prerelease"):
        raise RuntimeError("refuse draft/prerelease tool version")
    if key == "kustomize" and not release["tag_name"].startswith("kustomize/"):
        releases = metadata(f"https://api.github.com/repos/{repository}/releases?per_page=30")
        release = next(r for r in releases if r["tag_name"].startswith("kustomize/") and not r["prerelease"])
    if key == "helm":
        tag = release["tag_name"]
        asset = {"name": f"helm-{tag}-linux-amd64.tar.gz",
                 "browser_download_url": f"https://get.helm.sh/helm-{tag}-linux-amd64.tar.gz"}
    else:
        asset = choose_asset(key, release["assets"])
    cache.mkdir(parents=True, exist_ok=True)
    # Several projects reuse an asset filename for every release. A cache key
    # tied to the immutable release URL prevents attributing old bytes to a new tag.
    url_key = hashlib.sha256(asset["browser_download_url"].encode()).hexdigest()[:20]
    path = cache / (url_key + "-" + asset["name"])
    request = urllib.request.Request(asset["browser_download_url"], headers={"User-Agent": "Boxoro-toolchain-lock/1"})
    if path.is_symlink():
        raise RuntimeError("release cache cannot contain symlinks")
    if not path.exists():
        fd, temporary = tempfile.mkstemp(prefix="download-", dir=cache)
        pending = Path(temporary)
        try:
            with os.fdopen(fd, "wb") as output, urllib.request.urlopen(request, timeout=120) as response:
                count = 0
                while data := response.read(1024 * 1024):
                    count += len(data)
                    if count > 512 * 1024 * 1024:
                        raise RuntimeError("binary artifact exceeds 512MiB ceiling")
                    output.write(data)
            actual = verify_asset(pending, asset)
            # Failed or unverified downloads must never become reusable cache
            # entries: a later maintainer retry needs a fresh complete response.
            os.replace(pending, path)
        finally:
            pending.unlink(missing_ok=True)
    else:
        actual = verify_asset(path, asset)
    expected = asset.get("digest", "")
    archive = "tar" if re.search(r"\.tar\.(gz|xz)$", path.name) else "zip" if path.name.endswith(".zip") else "gz" if path.name.endswith(".gz") else "raw"
    member = None
    if archive in ("tar", "zip"):
        if archive == "tar":
            with tarfile.open(path, "r:*") as file:
                names = [m.name for m in file.getmembers() if m.isfile()]
        else:
            with zipfile.ZipFile(path) as file:
                names = [m.filename for m in file.infolist() if not m.is_dir()]
        matches = [name for name in names if Path(name).name == install["executable"]]
        if len(matches) != 1:
            raise RuntimeError("release lacks one exact executable member")
        member = matches[0]
    # Installer cache is addressed by content digest.
    cached = cache / actual
    if cached.is_symlink():
        raise RuntimeError("artifact cache cannot contain symlinks")
    if cached.exists():
        verify_asset(cached, {"digest": "sha256:" + actual, "size": path.stat().st_size})
    else:
        cached.hardlink_to(path)
    return {"kind": "binary", "package": repository, "version": release["tag_name"],
            "executable": install["executable"], "url": asset["browser_download_url"],
            "sha256": actual, "size": path.stat().st_size, "archive": archive, "member": member,
            "upstream_digest_verified": bool(expected), "release_url": release["html_url"]}


def resolve(key: str, tool: dict, cache: Path) -> dict:
    install = tool["install"]
    kind = install["kind"]
    if kind == "python":
        return resolve_python(key, tool)
    if kind == "npm":
        return resolve_npm(key, tool)
    if kind == "binary":
        return resolve_binary(key, tool, cache)
    if kind == "apt":
        return {"kind": "apt", "package": install["package"], "version": "Ubuntu noble repositories",
                "executable": install["executable"], "path": "/usr/bin/" + install["executable"]}
    if kind == "builtin":
        return {"kind": "builtin", "package": install["package"], "version": install.get("version", "1"),
                "executable": install["executable"], "path": "/opt/box/bin/" + install["executable"]}
    raise RuntimeError("unsupported installation kind")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--kinds", default="python,npm,binary,apt,builtin")
    parser.add_argument("--include-optional", action="store_true")
    parser.add_argument("--tools")
    parser.add_argument("--refresh", action="store_true")
    parser.add_argument("--cache", type=Path, required=True)
    args = parser.parse_args()
    catalog = json.loads((ROOT / "catalog.json").read_text())
    path = ROOT / "install.lock.json"
    lock = json.loads(path.read_text()) if path.exists() else {"schema_version": 1, "tools": {}}
    kinds = set(args.kinds.split(","))
    selected = set(args.tools.split(",")) if args.tools else set(catalog["tools"])
    levels = {"image", "optional"} if args.include_optional else {"image"}
    work = {key: tool for key, tool in catalog["tools"].items() if key in selected
            and tool["availability"] in levels and tool["install"]["kind"] in kinds
            and (args.refresh or key not in lock["tools"])}
    failures = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, min(args.jobs, 8))) as pool:
        futures = {pool.submit(resolve, key, tool, args.cache): key for key, tool in work.items()}
        for future in concurrent.futures.as_completed(futures):
            key = futures[future]
            try:
                lock["tools"][key] = future.result()
                print(f"LOCKED {key} {lock['tools'][key].get('version', '')}", flush=True)
            except Exception as error:
                failures.append(key)
                print(f"FAILED {key}: {error}", file=sys.stderr, flush=True)
            lock.update(generated_at=datetime.now(timezone.utc).isoformat(), python="3.12",
                        platform="linux/amd64", resolution="uv Python wheel locks; npm integrity locks; verified release artifacts")
            path.write_text(json.dumps(lock, indent=2, sort_keys=True) + "\n")
    if failures:
        print("Unresolved tools: " + ", ".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
