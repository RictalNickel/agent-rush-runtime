"""Integrity and extraction tests for the build-time security tool installer."""
import hashlib
import io
import json
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

INSTALLER = Path(__file__).resolve().parents[1] / "deploy/security/install-tools.py"


class InstallerIntegrityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.cache = self.root / "cache"
        self.cache.mkdir()
        self.prefix = self.root / "installed"

    def archive(self, entries):
        out = io.BytesIO()
        with tarfile.open(fileobj=out, mode="w:gz") as archive:
            for name, kind, data in entries:
                member = tarfile.TarInfo(name)
                if kind == "link":
                    member.type = tarfile.SYMTYPE
                    member.linkname = data.decode()
                    archive.addfile(member)
                else:
                    member.size = len(data)
                    archive.addfile(member, io.BytesIO(data))
        return out.getvalue()

    def run_install(self, blob, *, claimed_hash=None, platform="linux/amd64", cache=True):
        digest = claimed_hash or hashlib.sha256(blob).hexdigest()
        if cache:
            (self.cache / digest).write_bytes(blob)
        lock = {
            "schema_version": 1,
            "supported_platforms": ["linux/amd64"],
            "tools": [{"name": "fixture", "version": "1.0", "profile": "core",
                       "kind": "tar", "executable": "fixture", "member": "fixture",
                       "artifacts": {"linux/amd64": {
                           "url": "https://github.com/example/example/releases/download/v1/fixture.tar.gz",
                           "sha256": digest, "size": len(blob)}}}],
        }
        lock_path = self.root / "lock.json"
        lock_path.write_text(json.dumps(lock))
        return subprocess.run(
            [sys.executable, str(INSTALLER), "--lock", str(lock_path), "--prefix", str(self.prefix),
             "--artifact-dir", str(self.cache), "--offline", "--profile", "core",
             "--platform", platform], capture_output=True, text=True, timeout=10, check=False,
        )

    def test_verified_archive_installs_only_selected_executable(self):
        blob = self.archive([("release/fixture", "file", b"verified executable\n"),
                             ("release/README", "file", b"not installed")])
        result = self.run_install(blob)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.prefix / "bin/fixture").read_bytes(), b"verified executable\n")
        self.assertEqual((self.prefix / "bin/fixture").stat().st_mode & 0o777, 0o755)
        self.assertFalse((self.prefix / "bin/README").exists())
        manifest = json.loads((self.prefix / "versions.json").read_text())
        self.assertEqual(manifest["tools"]["fixture"]["sha256"], hashlib.sha256(blob).hexdigest())

    def test_digest_mismatch_never_installs_cached_bytes(self):
        result = self.run_install(self.archive([("fixture", "file", b"tampered")]), claimed_hash="0" * 64)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256", result.stderr)
        self.assertFalse((self.prefix / "bin/fixture").exists())

    def test_path_traversal_symlinks_and_duplicate_members_are_rejected(self):
        cases = [
            [("../fixture", "file", b"bad")],
            [("fixture", "link", b"/etc/passwd")],
            [("a/fixture", "file", b"one"), ("b/fixture", "file", b"two")],
        ]
        for entries in cases:
            with self.subTest(entries=entries):
                result = self.run_install(self.archive(entries))
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((self.prefix / "bin/fixture").exists())
        self.assertFalse((self.root / "fixture").exists())

    def test_unsupported_platform_cannot_silently_use_amd64(self):
        result = self.run_install(self.archive([("fixture", "file", b"ok")]), platform="linux/arm64")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported platform", result.stderr)

    def test_missing_offline_artifact_fails_without_installing(self):
        result = self.run_install(self.archive([("fixture", "file", b"ok")]), cache=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("offline", result.stderr)
        self.assertFalse((self.prefix / "bin/fixture").exists())


if __name__ == "__main__":
    unittest.main()
