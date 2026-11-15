"""Build-time dependency verification and extraction boundaries."""
import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def installer():
    spec = importlib.util.spec_from_file_location("agent_install", ROOT / "deploy/agents/install.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class AgentInstallTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_locked_file_hash_is_checked_before_installation(self):
        install = installer()
        path = self.root / "requirements.lock"
        path.write_text("pytest==1.0\n")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        self.assertEqual(install.locked_file(self.root, "requirements.lock", digest), path)
        path.write_text("changed")
        with self.assertRaises(install.InstallError):
            install.locked_file(self.root, "requirements.lock", digest)

    def test_cached_binary_is_rechecked_before_each_reuse(self):
        install = installer()
        digest = hashlib.sha256(b"good").hexdigest()
        path = self.root / digest
        path.write_bytes(b"good")
        item = {"url": "https://github.com/example/tool/releases/download/v1/tool",
                "sha256": digest, "size": 4}
        self.assertEqual(install.artifact(item, self.root, True), path)
        path.write_bytes(b"evil")
        with self.assertRaisesRegex(install.InstallError, "cached binary digest"):
            install.artifact(item, self.root, True)

    def test_lock_path_cannot_escape_or_follow_a_symlink(self):
        install = installer()
        outside = self.root / "outside"
        outside.write_text("x")
        locks = self.root / "locks"
        locks.mkdir()
        (locks / "link").symlink_to(outside)
        for name in ("../outside", "/etc/passwd", "link"):
            with self.assertRaises(install.InstallError):
                install.locked_file(locks, name, "0" * 64)

    def test_extracts_exact_binary_without_archive_links(self):
        install = installer()
        archive = self.root / "ok.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            entry = tarfile.TarInfo("release/tool")
            entry.size = 4
            tar.addfile(entry, io.BytesIO(b"test"))
        dest = self.root / "tool"
        install.extract_binary(archive, "tar", "release/tool", dest)
        self.assertEqual(dest.read_bytes(), b"test")

    def test_rejects_traversal_duplicate_and_symlink_archive_members(self):
        install = installer()
        for variant in ("traversal", "duplicate", "symlink"):
            archive = self.root / (variant + ".tar")
            with tarfile.open(archive, "w") as tar:
                item = tarfile.TarInfo("tool")
                item.size = 1
                tar.addfile(item, io.BytesIO(b"x"))
                bad = tarfile.TarInfo("../escape" if variant == "traversal" else "tool")
                if variant == "symlink":
                    bad.type = tarfile.SYMTYPE
                    bad.linkname = "/etc/passwd"
                else:
                    bad.size = 1
                tar.addfile(bad, None if variant == "symlink" else io.BytesIO(b"x"))
            with self.assertRaises(install.InstallError):
                install.extract_binary(archive, "tar", "tool", self.root / "out")

    def test_family_selection_includes_shared_tools_and_no_host_claims(self):
        install = installer()
        catalog = {
            "agents": {"qa": {"family": "browser", "tools": ["one", "host", "shared"]}},
            "tools": {"one": {"availability": "image"}, "host": {"availability": "host"},
                      "shared": {"availability": "image"}, "optional": {"availability": "optional"}},
        }
        self.assertEqual(install.select_tools(catalog, "browser", None, False), ["one", "shared"])
        self.assertEqual(install.select_tools(catalog, "suite", None, True), ["one", "optional", "shared"])
        with self.assertRaises(install.InstallError):
            install.select_tools(catalog, "unknown", None, False)

    def test_lock_index_has_all_image_packages_and_verified_lockfiles(self):
        install = installer()
        catalog = json.loads((ROOT / "deploy/agents/catalog.json").read_text())
        lock = json.loads((ROOT / "deploy/agents/install.lock.json").read_text())
        for key, tool in catalog["tools"].items():
            if tool["availability"] != "image":
                continue
            self.assertIn(key, lock["tools"], key)
        for item in lock["tools"].values():
            for key in ("requirements", "build_requirements", "package_json", "package_lock"):
                if key in item:
                    install.locked_file(ROOT / "deploy/agents", item[key], item[key + "_sha256"])


if __name__ == "__main__":
    unittest.main()
