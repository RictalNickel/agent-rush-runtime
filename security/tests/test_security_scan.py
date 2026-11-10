"""Smoke tests for the offline-first scan wrapper (no scanner binaries required)."""
import importlib.machinery
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).parents[1] / "bin" / "security-scan"
loader = importlib.machinery.SourceFileLoader("security_scan", str(SCRIPT))
spec = importlib.util.spec_from_loader("security_scan", loader)
module = importlib.util.module_from_spec(spec)
loader.exec_module(module)

class DispatcherTests(unittest.TestCase):
    def test_known_profiles_are_present(self):
        self.assertTrue({"fast","code","secrets","supply-chain","config","malware-static"}.issubset(module.PROFILES))
    def test_no_shell_interpolation(self):
        self.assertEqual(module.command_for("ruff", Path("/workspace/a b")), ["ruff", "check", "--output-format", "json", "/workspace/a b"])
    def test_yara_requires_rules(self):
        self.assertIsNone(module.command_for("yara", Path("/workspace/code")))
    def test_shellcheck_skips_when_no_shell_scripts(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertIsNone(module.command_for("shellcheck", Path(d)))
    def test_invalid_target_rejected(self):
        with patch.object(sys, "argv", ["security-scan", "/etc", "--profile", "fast"]):
            with self.assertRaises(SystemExit):
                module.main()
    def test_rejects_output_outside_workspace(self):
        with patch.object(sys, "argv", ["security-scan", "/workspace", "--output", "/tmp/output.json"]):
            with self.assertRaises(SystemExit):
                module.main()

if __name__ == "__main__":
    unittest.main()
