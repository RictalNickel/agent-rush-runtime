"""Independent regressions for canonical commands and maintainer lock refresh."""

import hashlib
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
import zipfile
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

    def python_fixture(self):
        cache = self.root / "cache"
        wheels = cache / "wheels"
        wheels.mkdir(parents=True)
        wheel = wheels / "boxoro_install_fixture-1.0-py3-none-any.whl"
        original = b"LOCKED = True\n"
        files = {
            "boxoro_install_fixture/__init__.py": original,
            "boxoro_install_fixture-1.0.dist-info/METADATA":
                b"Metadata-Version: 2.1\nName: boxoro-install-fixture\nVersion: 1.0\n",
            "boxoro_install_fixture-1.0.dist-info/WHEEL":
                b"Wheel-Version: 1.0\nGenerator: fixture\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        }
        files["boxoro_install_fixture-1.0.dist-info/RECORD"] = (
            "".join(name + ",,\n" for name in files)
            + "boxoro_install_fixture-1.0.dist-info/RECORD,,\n"
        ).encode()
        with zipfile.ZipFile(wheel, "w") as archive:
            for name, data in files.items():
                archive.writestr(name, data)
        requirements = self.root / "fixture.lock"
        requirements.write_text("boxoro-install-fixture==1.0 --hash=sha256:"
                                + hashlib.sha256(wheel.read_bytes()).hexdigest() + "\n")
        item = {"kind": "python", "library": True, "package": "boxoro-install-fixture",
                "requirements": "fixture.lock",
                "requirements_sha256": hashlib.sha256(requirements.read_bytes()).hexdigest()}
        return item, cache, wheel, original

    def test_python_reinstall_replaces_modified_and_unlocked_files(self):
        install = module("install")
        item, cache, _, original = self.python_fixture()
        prefix = self.root / "prefix"
        install.install_tool("fixture", item, self.root, prefix, cache, True, sys.executable, "unused")
        python = prefix / "python/fixture/bin/python"
        site = Path(subprocess.check_output(
            [str(python), "-I", "-c", "import sysconfig; print(sysconfig.get_path('purelib'))"],
            text=True, timeout=5).strip())
        installed = site / "boxoro_install_fixture/__init__.py"
        installed.write_text("LOCKED = False\n")
        leftover = site / "unlocked_old_dependency.py"
        leftover.write_text("STALE = True\n")

        install.install_tool("fixture", item, self.root, prefix, cache, True, sys.executable, "unused")

        self.assertEqual(installed.read_bytes(), original)
        self.assertFalse(leftover.exists())

    def test_offline_python_wheel_still_requires_its_locked_hash(self):
        install = module("install")
        item, cache, wheel, _ = self.python_fixture()
        # A ZIP still opens with appended data, but the complete artifact hash
        # must match; a reusable download directory is not a trust bypass.
        with wheel.open("ab") as output:
            output.write(b"modified after lock review")
        prefix = self.root / "prefix"
        with self.assertRaises(subprocess.CalledProcessError):
            install.install_tool("fixture", item, self.root, prefix, cache, True, sys.executable, "unused")
        self.assertFalse((prefix / "bin/fixture").exists())

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

    def test_interrupted_release_download_can_retry_without_poisoning_cache(self):
        resolver = module("resolve-locks")
        release = {"tag_name": "v-new", "html_url": "https://github.com/hadolint/hadolint/releases/tag/v-new",
                   "assets": [{"name": "hadolint-Linux-x86_64", "size": 11,
                               "browser_download_url": "https://github.com/hadolint/hadolint/releases/download/v-new/hadolint-Linux-x86_64"}]}
        tool = {"install": {"package": "hadolint/hadolint", "executable": "hadolint"}}

        class Interrupted(io.BytesIO):
            def read(self, count):
                if self.tell():
                    raise OSError("interrupted fixture download")
                return super().read(3)

        with patch.object(resolver, "metadata", return_value=release), \
             patch.object(resolver.urllib.request, "urlopen",
                          side_effect=[Interrupted(b"new-version"), io.BytesIO(b"new-version")]):
            with self.assertRaisesRegex(OSError, "interrupted fixture"):
                resolver.resolve_binary("hadolint", tool, self.root)
            self.assertEqual(list(self.root.iterdir()), [])
            item = resolver.resolve_binary("hadolint", tool, self.root)

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
