"""Independent regressions for canonical commands and maintainer lock refresh."""

import hashlib
import importlib.util
import io
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), ROOT / "deploy/agents" / (name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


class InstallerReviewTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_system_alias_preserves_canonical_command_name(self):
        install = module("install")
        item = {"kind": "apt", "path": "/bin/sh", "executable": "sh"}
        install.install_tool("different-name", item, self.root, self.root, self.root, True, "unused", "unused")
        result = subprocess.run([str(self.root / "bin/different-name"), "-c", 'printf "%s" "$0"'],
                                capture_output=True, text=True, check=True, timeout=5)
        self.assertEqual(result.stdout, "/bin/sh")

    def test_release_refresh_does_not_reuse_same_named_old_asset(self):
        resolver = module("resolve-locks")
        filename = "hadolint-Linux-x86_64"
        (self.root / filename).write_bytes(b"old-version")
        release = {"tag_name": "v-new", "html_url": "https://github.com/hadolint/hadolint/releases/tag/v-new",
                   "assets": [{"name": filename, "size": 11,
                               "browser_download_url": "https://github.com/hadolint/hadolint/releases/download/v-new/" + filename}]}
        with patch.object(resolver, "metadata", return_value=release), \
             patch.object(resolver.urllib.request, "urlopen", return_value=io.BytesIO(b"new-version")) as download:
            item = resolver.resolve_binary("hadolint", {"install": {"package": "hadolint/hadolint", "executable": "hadolint"}}, self.root)
        self.assertEqual(download.call_count, 1)
        self.assertEqual(item["version"], "v-new")
        self.assertEqual(item["sha256"], hashlib.sha256(b"new-version").hexdigest())
        self.assertEqual((self.root / item["sha256"]).read_bytes(), b"new-version")

    def test_npm_install_script_approvals_are_versioned_and_reject_new_scripts(self):
        resolver = module("resolve-locks")
        lock = {"packages": {"": {}, "node_modules/esbuild": {"version": "1.2.3", "hasInstallScript": True},
                             "node_modules/nested/node_modules/fsevents": {"version": "4.5.6", "hasInstallScript": True}}}
        self.assertEqual(resolver.script_approvals("promptfoo", lock), {"esbuild@1.2.3": True, "fsevents@4.5.6": False})
        lock["packages"]["node_modules/nested/node_modules/unreviewed"] = {"version": "1.0.0", "hasInstallScript": True}
        with self.assertRaises(RuntimeError):
            resolver.script_approvals("promptfoo", lock)

    def test_npm_refresh_resolves_without_old_lock_and_preserves_it_on_failure(self):
        resolver = module("resolve-locks")
        resolver.ROOT = self.root
        directory = self.root / "locks/npm/example"
        directory.mkdir(parents=True)
        previous = '{"old":"reviewed lock"}\n'
        (directory / "package-lock.json").write_text(previous)
        (directory / "package.json").write_text('{"old":"reviewed manifest"}\n')
        tool = {"install": {"package": "example", "executable": "example"}}

        def resolve_command(argv, *, cwd):
            self.assertFalse((cwd / "package-lock.json").exists())
            value = {"packages": {"": {"dependencies": {"example": "*"}},
                                  "node_modules/example": {"version": "2.0.0", "bin": {"example": "index.js"}}}}
            (cwd / "package-lock.json").write_text(json.dumps(value))

        with patch.object(resolver, "command", side_effect=resolve_command):
            result = resolver.resolve_npm("example", tool)
        self.assertEqual(result["version"], "2.0.0")
        reviewed = {path.name: path.read_bytes() for path in directory.glob("*.json")}
        with patch.object(resolver, "command", side_effect=RuntimeError("registry unavailable")):
            with self.assertRaises(RuntimeError):
                resolver.resolve_npm("example", tool)
        self.assertEqual({path.name: path.read_bytes() for path in directory.glob("*.json")}, reviewed)

        def missing_executable(argv, *, cwd):
            value = {"packages": {"": {"dependencies": {"example": "*"}},
                                  "node_modules/example": {"version": "3.0.0", "bin": {"other": "index.js"}}}}
            (cwd / "package-lock.json").write_text(json.dumps(value))

        with patch.object(resolver, "command", side_effect=missing_executable):
            with self.assertRaises(RuntimeError):
                resolver.resolve_npm("example", tool)
        self.assertEqual({path.name: path.read_bytes() for path in directory.glob("*.json")}, reviewed)


if __name__ == "__main__":
    unittest.main()
