#!/usr/bin/env python3
"""Install the checksum-pinned Node distribution required by specialist tools."""
import argparse
import json
import shutil
import tarfile
import tempfile
from pathlib import Path

from install import InstallError, artifact, safe_member


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prefix", type=Path, default=Path("/opt/box/node"))
    parser.add_argument("--cache", type=Path, default=Path("/tmp/boxoro-agent-downloads/binaries"))
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    lock = json.loads(Path(__file__).with_name("node.lock.json").read_text())
    archive = artifact(lock, args.cache, args.offline)
    if args.prefix.exists():
        raise InstallError("Node destination already exists; use a fresh build stage")
    args.prefix.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="node-stage-", dir=args.prefix.parent) as temporary:
        with tarfile.open(archive, "r:xz") as source:
            for member in source.getmembers():
                safe_member(member.name)
                if not member.name.startswith(lock["prefix"] + "/") and member.name != lock["prefix"]:
                    raise InstallError("Node archive has an unexpected root")
            # Python's data filter checks symlink destinations, device types,
            # owner bits and traversal; the published distribution checksum is pinned.
            source.extractall(temporary, filter="data")
        shutil.move(str(Path(temporary) / lock["prefix"]), args.prefix)
    print("Installed Node " + lock["version"])


if __name__ == "__main__":
    main()
