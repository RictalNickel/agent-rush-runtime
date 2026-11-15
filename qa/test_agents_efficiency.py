"""Task-scoped discovery/readiness must preserve honest, bounded coverage."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

from qa.test_agents_runtime import fixture_catalog

from boxoro_agents.runner import run_request
from boxoro_agents.process import ProcessResult
from boxoro_agents.qualification import qualify_result


class ScopedSpecialistTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.probes = self.root / "probes.txt"
        self.catalog = fixture_catalog()
        self.catalog["tools"]["unused"] = {**self.catalog["tools"]["fixture"], "availability": "optional"}
        self.catalog["agents"]["testforge"]["tools"].append("unused")
        self.catalog["operations"]["inspect"]["effects"] = ["network"]
        self.catalog["operations"]["second"]["effects"] = ["project_code"]
        self.catalog_path = self.root / "catalog.json"
        self.catalog_path.write_text(json.dumps(self.catalog))
        for name in ("fixture", "unused"):
            executable = self.bin / name
            executable.write_text(
                "#!" + sys.executable + "\nfrom pathlib import Path\n"
                + "with Path(" + repr(str(self.probes)) + ").open('a') as stream: stream.write("
                + repr(name + "\n") + ")\nprint('version 1.0')\n"
            )
            executable.chmod(0o700)

    def request(self, action, **scope):
        return run_request(
            {"action": action, "agent": "testforge", **scope},
            workspace=self.workspace, tool_bin=self.bin, catalog_path=self.catalog_path,
        )

    def test_describe_operation_returns_only_its_contract_without_probing(self):
        result = self.request("describe", operation="inspect")
        self.assertEqual(set(result["operations"]), {"inspect"})
        self.assertEqual(set(result["tools"]), {"fixture"})
        self.assertEqual(result["scope"]["kind"], "operation")
        self.assertEqual(result["scope"]["excluded_tools"], ["unused"])
        self.assertEqual(result["required_effects"], ["network"])
        self.assertNotIn("skill", result["definition"])
        self.assertFalse(self.probes.exists())
        self.assertFalse((self.workspace / ".specialists").exists())

    def test_describe_workflow_keeps_order_and_collects_all_effects(self):
        result = self.request("describe", workflow="check")
        self.assertEqual(list(result["operations"]), ["inspect", "second"])
        self.assertEqual(set(result["required_effects"]), {"network", "project_code"})
        self.assertEqual(result["definition"]["workflows"], {"check": ["inspect", "second"]})
        self.assertFalse(self.probes.exists())

    def test_doctor_probes_only_selected_tool_even_when_optional_peer_is_missing(self):
        (self.bin / "unused").unlink()
        result = self.request("doctor", operation="inspect")
        self.assertEqual(result["gate"], "pass")
        self.assertEqual([c["tool"] for c in result["checks"]], ["fixture"])
        self.assertEqual(self.probes.read_text().splitlines(), ["fixture"])
        self.assertEqual(result["scope"]["operations"], ["inspect"])
        stored = json.loads((self.workspace / result["reports"]["json"]).read_text())
        self.assertEqual(stored["scope"], result["scope"])
        full = self.request("doctor")
        self.assertEqual(full["gate"], "incomplete")
        self.assertEqual(len(full["checks"]), 2)

    def test_workflow_doctor_deduplicates_shared_tools_without_running_operations(self):
        result = self.request("doctor", workflow="check")
        self.assertEqual(result["gate"], "pass")
        self.assertEqual(result["scope"]["operations"], ["inspect", "second"])
        self.assertEqual(self.probes.read_text().splitlines(), ["fixture"])
        self.assertEqual(len(result["checks"]), 1)

    def test_scope_rejects_ambiguous_unknown_and_cross_profile_selections(self):
        for action in ("describe", "doctor"):
            for selection in ({"operation": "inspect", "workflow": "check"},
                              {"operation": "unknown"}, {"workflow": "unknown"},
                              {"agent": "other", "operation": "inspect"}):
                with self.subTest(action=action, selection=selection), self.assertRaises(ValueError):
                    self.request(action, **selection)
        self.assertFalse(self.probes.exists())

    def test_unscoped_description_preserves_full_profile_and_all_operations(self):
        result = self.request("describe")
        self.assertEqual(set(result["tools"]), {"fixture", "unused"})
        self.assertEqual(set(result["operations"]), {"inspect", "second"})
        self.assertEqual(result["definition"]["skill"], "Run reviewed tests.")

    def test_directory_only_operation_rejects_file_before_launch_or_reports(self):
        self.catalog["operations"]["inspect"]["target_type"] = "directory"
        self.catalog_path.write_text(json.dumps(self.catalog))
        (self.workspace / "README.md").write_text("# Selected file\n")
        with self.assertRaisesRegex(ValueError, "directory"):
            self.request("run", operation="inspect", path="README.md", allow_network=True)
        self.assertFalse(self.probes.exists())
        self.assertFalse((self.workspace / ".specialists").exists())
        self.assertEqual(self.request("run", operation="inspect", path=".", allow_network=True)["gate"], "pass")

    def test_unknown_catalog_target_type_is_rejected(self):
        self.catalog["operations"]["inspect"]["target_type"] = "anything"
        self.catalog_path.write_text(json.dumps(self.catalog))
        with self.assertRaises(ValueError):
            self.request("describe")


class NativeSummaryQualificationTests(unittest.TestCase):
    def qualify(self, operation, tool, stdout, stderr="", code=0):
        check = {"operation": operation, "tool": tool, "status": "passed" if code == 0 else "findings", "artifacts": []}
        process = ProcessResult("complete", code, stdout.encode(), stderr.encode(), 0.01)
        qualify_result(check, process, Path("unused"), Path("unused"))
        return check

    def test_markdownlint_requires_positive_file_coverage_and_consistent_findings(self):
        for operation in ("markdownlint-check", "markdownlint-file"):
            for files, issues, code, expected in [(0, 0, 0, "error"), (1, 0, 0, "passed"),
                                                  (1, 2, 1, "findings"), (1, 0, 1, "error"),
                                                  (1, 2, 0, "findings")]:
                with self.subTest(operation=operation, files=files, issues=issues, code=code):
                    output = f"Linting: {files} file" + ("s" if files != 1 else "")
                    output += f"\nSummary: {issues} issues in " + ("1 file\n" if issues else "0 files\n")
                    check = self.qualify(operation, "markdownlint", output, code=code)
                    self.assertEqual(check["status"], expected)
                    self.assertEqual(check["qualification"]["scanned_files"], files)
        self.assertEqual(self.qualify("markdownlint-check", "markdownlint", "unexpected output")["status"], "error")

    def test_i18next_status_requires_completed_nonempty_analysis(self):
        for output, error, code, expected in [
            ("Keys Found: 1\n", "✔ Analysis complete.\n", 0, "passed"),
            ("Keys Found: 1\n", "✔ Analysis complete.\n", 1, "findings"),
            ("Keys Found: 0\n", "✔ Analysis complete.\n", 0, "error"),
            ("", "Error loading configuration: SyntaxError\n", 1, "error"),
            ("Keys Found: 1\n", "", 0, "error"),
            ("Keys Found: 1\n", "Skipping file due to error: src/broken.ts\n✔ Analysis complete.\n", 0, "error"),
            ("Keys Found: 1\n", "Skipping file in constants pre-scan due to error: src/broken.ts\n✔ Analysis complete.\n", 0, "error"),
            ("Keys Found: 1\n", "Failed to process file src/broken.ts\n✔ Analysis complete.\n", 0, "error"),
            ("Keys Found: 1\n", "Plugin fixture onVisitNode failed: Error\n✔ Analysis complete.\n", 0, "error"),
            ("Keys Found: 1\n", "Plugin fixture extractKeysFromExpression failed: Error\n✔ Analysis complete.\n", 0, "error"),
            ("Keys Found: 1\n", "Plugin fixture extractContextFromExpression failed: Error\n✔ Analysis complete.\n", 0, "error"),
        ]:
            with self.subTest(output=output, code=code):
                self.assertEqual(self.qualify("i18next-status", "i18next", output, error, code)["status"], expected)

    def test_i18next_unused_keeps_parser_failures_incomplete(self):
        clean = "Summary: 🎉 No unused keys found.\n"
        found = "Summary: Found 2 unused key(s). No files were modified.\n"
        parse_error = "⚠ Some source files could not be parsed — keys may be falsely reported as unused.\n"
        for output, error, code, expected in [
            (clean, "✔ Analysis complete.\n", 0, "passed"),
            (found, "✔ Analysis complete.\n", 1, "findings"),
            (parse_error + found, "✔ Analysis complete.\n", 1, "error"),
            ("", "Error loading configuration: SyntaxError\n", 1, "error"),
            (clean, "✔ Analysis complete.\n", 1, "error"),
        ]:
            with self.subTest(output=output, code=code):
                self.assertEqual(self.qualify("i18next-unused", "i18next", output, error, code)["status"], expected)

    def test_buf_breaking_requires_compatibility_diagnostics_for_findings(self):
        breaking = json.dumps({"type": "FIELD_SAME_TYPE", "message": "Field type changed.", "path": "item.proto"}) + "\n"
        compile_error = json.dumps({"type": "COMPILE", "message": "Invalid proto syntax.", "path": "item.proto"}) + "\n"
        for output, code, expected in [("", 0, "passed"), (breaking, 100, "findings"),
                                       (compile_error, 100, "error"), ("", 100, "error"),
                                       ("not JSON", 100, "error")]:
            with self.subTest(output=output, code=code):
                self.assertEqual(self.qualify("buf-breaking", "buf", output, code=code)["status"], expected)


if __name__ == "__main__":
    unittest.main()
