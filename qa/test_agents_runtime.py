"""Specialist runtime boundaries, using real subprocesses and local fixtures."""

import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPOSITORY / "deploy/agents"))

try:
    from boxoro_agents.catalog import Catalog
    from boxoro_agents.policy import Request
    from boxoro_agents.process import run_process
    from boxoro_agents.runner import parse_json, run_request
except ImportError:
    Catalog = None

try:
    from boxoro_agents.builtins import run_analysis
except ImportError:
    run_analysis = None


def fixture_catalog():
    return {
        "schema_version": 1,
        "agents": {
            "testforge": {
                "name": "TestForge",
                "description": "Fixture test specialist",
                "family": "test",
                "skill": "Run reviewed tests.",
                "tools": ["fixture"],
                "workflows": {"check": ["inspect", "second"]},
                "connectors": [],
                "resources": {"memory_mb": 256, "disk_mb": 256},
                "limitations": [],
            },
            "other": {
                "name": "Other",
                "description": "Different tool membership",
                "family": "other",
                "skill": "Different tools.",
                "tools": [],
                "workflows": {},
                "connectors": [],
                "resources": {"memory_mb": 256, "disk_mb": 256},
                "limitations": [],
            },
        },
        "tools": {
            "fixture": {
                "name": "Fixture",
                "description": "Real subprocess fixture",
                "homepage": "https://example.test/fixture",
                "sources": ["https://example.test/fixture"],
                "license": "MIT",
                "family": "test",
                "install": {"kind": "builtin", "executable": "fixture"},
                "probe": ["--version"],
                "availability": "image",
            }
        },
        "operations": {
            "inspect": {
                "tool": "fixture",
                "description": "Inspect arguments",
                "argv": ["inspect", "{path}", "{value}", "{artifacts}"],
                "parameters": {
                    "value": {"type": "string", "default": "hello", "maxLength": 128}
                },
                "effects": [],
                "success_codes": [0],
                "finding_codes": [1],
                "timeout_seconds": 10,
                "notes": [],
            },
            "second": {
                "tool": "fixture",
                "description": "Second step",
                "argv": ["second"],
                "parameters": {},
                "effects": [],
                "success_codes": [0],
                "finding_codes": [1],
                "timeout_seconds": 10,
                "notes": [],
            },
        },
        "connectors": {},
    }


class SpecialistRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(Catalog, "specialist runtime contract is not implemented")
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        self.project = self.workspace / "app"
        self.project.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.catalog_path = self.root / "catalog.json"
        self.catalog = fixture_catalog()
        self.write_catalog()
        self.make_tool()

    def write_catalog(self):
        self.catalog_path.write_text(json.dumps(self.catalog))

    def make_tool(self, body=None):
        executable = self.bin / "fixture"
        executable.write_text(
            "#!" + sys.executable + "\n"
            + (body or (
                "import json, os, sys\n"
                "if sys.argv[1:] == ['--version']:\n"
                " print('fixture 1.0'); raise SystemExit(0)\n"
                "print(json.dumps({'argv':sys.argv[1:], 'cwd':os.getcwd(), "
                "'secret':os.getenv('BOXORO_TEST_SECRET','absent'), "
                "'chrome':os.getenv('CHROME_PATH'), 'puppeteer':os.getenv('PUPPETEER_EXECUTABLE_PATH'), "
                "'httpie_config':os.getenv('HTTPIE_CONFIG_DIR')}))\n"
            ))
        )
        executable.chmod(0o700)
        return executable

    def request(self, **kwargs):
        value = {"action": "run", "agent": "testforge", "operation": "inspect", "path": "app"}
        value.update(kwargs)
        return run_request(value, workspace=self.workspace, tool_bin=self.bin, catalog_path=self.catalog_path)

    def test_json_rejects_duplicates_nonfinite_values_and_oversize(self):
        for raw in [
            '{"action":"list","action":"run"}',
            '{"action":"run","parameters":{"x":NaN}}',
            '{"action":"run","parameters":{"x":Infinity}}',
            '{"action":"run","parameters":{"x":1e999}}',
            '"' + "x" * 65536 + '"',
        ]:
            with self.subTest(raw=raw[:60]), self.assertRaises(ValueError):
                parse_json(raw)

    def test_request_rejects_unknown_cross_action_and_wrong_types(self):
        for value in [
            {"action": "list", "command": "id"},
            {"action": "list", "agent": "testforge"},
            {"action": "describe"},
            {"action": "doctor", "parameters": {}},
            {"action": "doctor"},
            {"action": "doctor", "agent": "testforge", "path": "app"},
            {"action": "run", "agent": "testforge"},
            {"action": "run", "agent": "testforge", "operation": "inspect", "timeout_seconds": True},
            {"action": "run", "agent": "testforge", "operation": "inspect", "timeout_seconds": 541},
            {"action": "run", "agent": "testforge", "operation": "inspect", "allow_network": "false"},
            {"action": "run", "agent": "testforge", "operation": "inspect", "parameters": []},
        ]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                Request.parse(value)

    def test_catalog_rejects_unknown_placeholder_and_cross_agent_workflow(self):
        self.catalog["operations"]["inspect"]["argv"].append("{unregistered}")
        self.write_catalog()
        with self.assertRaises(ValueError):
            Catalog.load(self.catalog_path)
        self.catalog = fixture_catalog()
        self.catalog["agents"]["other"]["workflows"] = {"unsafe": ["inspect"]}
        self.write_catalog()
        with self.assertRaises(ValueError):
            Catalog.load(self.catalog_path)

    def test_list_and_describe_include_only_registered_operations(self):
        listed = run_request({"action": "list"}, workspace=self.workspace, catalog_path=self.catalog_path)
        self.assertEqual({a["id"] for a in listed["agents"]}, {"testforge", "other"})
        described = run_request({"action": "describe", "agent": "testforge"}, workspace=self.workspace, catalog_path=self.catalog_path)
        self.assertEqual(set(described["operations"]), {"inspect", "second"})

    def test_cross_agent_operation_is_refused(self):
        with self.assertRaises(ValueError):
            self.request(agent="other")

    def test_unknown_or_option_shaped_parameter_never_launches(self):
        for parameters in [{"command": "id"}, {"value": "--config=evil"}, {"value": "x\0y"}]:
            with self.subTest(parameters=parameters), self.assertRaises(ValueError):
                self.request(parameters=parameters)

    def test_argument_metacharacters_are_literal_and_no_environment_secrets(self):
        os.environ["BOXORO_TEST_SECRET"] = "must-not-be-in-child"
        self.addCleanup(os.environ.pop, "BOXORO_TEST_SECRET", None)
        previous_chrome = os.environ.get("CHROME_PATH")
        os.environ["CHROME_PATH"] = str(self.project / "untrusted-chrome")
        self.addCleanup(lambda: os.environ.pop("CHROME_PATH", None) if previous_chrome is None else os.environ.__setitem__("CHROME_PATH", previous_chrome))
        result = self.request(parameters={"value": "$(touch escaped); `whoami`"})
        self.assertEqual(result["gate"], "pass")
        output = json.loads((self.workspace / result["checks"][0]["reports"]["stdout"]).read_text())
        self.assertEqual(output["argv"][2], "$(touch escaped); `whoami`")
        self.assertEqual(output["argv"][1], str(self.project))
        self.assertEqual(output["secret"], "absent")
        self.assertEqual(output["chrome"], "/opt/box/chromium")
        self.assertEqual(output["puppeteer"], "/opt/box/chromium")
        self.assertEqual(output["httpie_config"], "/opt/box/agents/boxoro_agents/httpie-config")
        self.assertFalse((self.project / "escaped").exists())
        self.assertNotIn("stdout", result["checks"][0])
        self.assertTrue((self.workspace / result["reports"]["json"]).is_file())

    def test_file_target_preserves_input_and_uses_parent_working_directory(self):
        data = self.project / "data.csv"
        data.write_text("value\n1\n")
        result = self.request(path="app/data.csv")
        self.assertEqual(result["gate"], "pass")
        output = json.loads((self.workspace / result["checks"][0]["reports"]["stdout"]).read_text())
        self.assertEqual(output["argv"][1], str(data))
        self.assertEqual(output["cwd"], str(self.project))

    def test_workspace_and_parameter_paths_cannot_escape_through_symlinks(self):
        (self.workspace / "escape").symlink_to(self.root)
        for path in ["../", str(self.root), "escape", "app/../", ".specialists/reports"]:
            with self.subTest(path=path), self.assertRaises(ValueError):
                self.request(path=path)
        self.catalog["operations"]["inspect"]["parameters"]["value"]["format"] = "path"
        self.write_catalog()
        (self.project / "outside").symlink_to(self.root)
        with self.assertRaises(ValueError):
            self.request(parameters={"value": "outside"})

    def test_effect_declarations_block_before_running(self):
        marker = self.project / "ran"
        self.make_tool("from pathlib import Path\nPath(" + repr(str(marker)) + ").touch()\n")
        self.catalog["operations"]["inspect"]["effects"] = ["project_code", "network", "mutation"]
        self.write_catalog()
        blocked = self.request()
        self.assertEqual(blocked["gate"], "incomplete")
        self.assertEqual(blocked["checks"][0]["status"], "blocked")
        self.assertFalse(marker.exists())
        allowed = self.request(allow_project_code=True, allow_network=True, allow_mutation=True)
        self.assertEqual(allowed["gate"], "pass")
        self.assertTrue(marker.exists())

    def test_missing_binary_or_failed_probe_never_reports_ready(self):
        (self.bin / "fixture").unlink()
        result = self.request()
        self.assertEqual(result["gate"], "incomplete")
        self.assertEqual(result["checks"][0]["status"], "missing")
        self.make_tool("raise SystemExit(3)\n")
        doctor = run_request({"action": "doctor", "agent": "testforge"}, workspace=self.workspace, tool_bin=self.bin, catalog_path=self.catalog_path)
        self.assertEqual(doctor["gate"], "incomplete")
        self.assertEqual(doctor["checks"][0]["status"], "error")

    def test_doctor_distinguishes_probe_readiness_from_registered_operations(self):
        self.catalog["tools"]["library"] = dict(self.catalog["tools"]["fixture"])
        self.catalog["agents"]["testforge"]["tools"].append("library")
        self.write_catalog()
        (self.bin / "library").symlink_to(self.bin / "fixture")
        result = run_request({"action": "doctor", "agent": "testforge"}, workspace=self.workspace, tool_bin=self.bin, catalog_path=self.catalog_path)
        checks = {check["tool"]: check for check in result["checks"]}
        self.assertEqual(checks["fixture"]["runnable_operations"], 2)
        self.assertEqual(checks["library"]["runnable_operations"], 0)
        self.assertEqual(checks["library"]["capability"], "library_or_cli")

    def test_doctor_accepts_only_registered_nonzero_probe_codes(self):
        self.catalog["tools"]["fixture"]["probe_success_codes"] = [64]
        self.write_catalog()
        self.make_tool("raise SystemExit(64)\n")
        result = run_request({"action": "doctor", "agent": "testforge"}, workspace=self.workspace, tool_bin=self.bin, catalog_path=self.catalog_path)
        self.assertEqual(result["gate"], "pass")
        self.catalog["tools"]["fixture"]["probe_success_codes"] = [True]
        self.write_catalog()
        with self.assertRaises(ValueError):
            Catalog.load(self.catalog_path)

    def test_huge_integer_number_parameter_does_not_overflow_validation(self):
        self.catalog["operations"]["inspect"]["parameters"]["value"] = {"type": "number", "minimum": 0, "maximum": 100}
        self.write_catalog()
        with self.assertRaises(ValueError):
            self.request(parameters={"value": 10 ** 400})

    def test_invalid_catalog_parameter_constraints_are_rejected(self):
        for constraint in [{"unknownValidation": True}, {"maxLength": -1}, {"format": "path", "type": "integer"}, {"minimum": "unbounded"}]:
            self.catalog = fixture_catalog()
            self.catalog["operations"]["inspect"]["parameters"]["value"].update(constraint)
            self.write_catalog()
            with self.subTest(constraint=constraint), self.assertRaises(ValueError):
                Catalog.load(self.catalog_path)

    def test_findings_and_operational_errors_have_different_gates(self):
        for code, gate, status in [(0, "pass", "passed"), (1, "fail", "findings"), (2, "incomplete", "error"), (5, "incomplete", "error")]:
            with self.subTest(code=code):
                self.make_tool("raise SystemExit(" + str(code) + ")\n")
                result = self.request()
                self.assertEqual(result["gate"], gate)
                self.assertEqual(result["checks"][0]["status"], status)

    def test_descriptive_metric_notes_survive_a_successful_command(self):
        self.catalog["operations"]["inspect"]["notes"] = "This command records metrics without enforcing a quality threshold."
        self.write_catalog()
        result = self.request()
        self.assertEqual(result["checks"][0]["notes"], "This command records metrics without enforcing a quality threshold.")

    def test_workflow_stops_on_findings_and_only_passes_referenced_parameters(self):
        self.make_tool("import sys\nprint(sys.argv[1:])\nraise SystemExit(1)\n")
        result = self.request(action="workflow", operation=None, workflow="check", parameters={"value": "one"})
        self.assertEqual(result["gate"], "fail")
        self.assertEqual(len(result["checks"]), 1)
        self.assertEqual(result["remaining_operations"], ["second"])
        self.make_tool()
        result = self.request(action="workflow", operation=None, workflow="check", parameters={"value": "one"})
        self.assertEqual(result["gate"], "pass")
        self.assertEqual(len(result["checks"]), 2)

    def test_native_artifact_symlinks_are_not_published(self):
        self.make_tool("import pathlib,sys\npathlib.Path(sys.argv[-1], 'outside').symlink_to('/etc/passwd')\n")
        result = self.request()
        self.assertEqual(result["gate"], "incomplete")
        self.assertEqual(result["checks"][0]["status"], "artifact_limit")
        self.assertFalse(any(x.get("path", "").endswith("/outside") for x in result["checks"][0]["artifacts"]))

    def test_native_artifact_root_cannot_be_replaced_by_a_symlink(self):
        outside = self.root / "outside"
        outside.mkdir()
        (outside / "sensitive.txt").write_text("not a native report")
        self.make_tool("import pathlib,sys\np=pathlib.Path(sys.argv[-1])\np.rmdir()\np.symlink_to(" + repr(str(outside)) + ")\n")
        result = self.request()
        self.assertEqual(result["gate"], "incomplete")
        self.assertEqual(result["checks"][0]["artifacts"], [])

    def test_native_report_writer_refuses_symlink_parent(self):
        from boxoro_agents.reports import private_write
        outside = self.root / "outside"
        outside.mkdir()
        link = self.project / "redirect"
        link.symlink_to(outside)
        with self.assertRaises(OSError):
            private_write(link / "report.txt", b"private report")
        self.assertFalse((outside / "report.txt").exists())

    def qualified_tool(self, operation_id, tool_id, body):
        self.catalog = fixture_catalog()
        self.catalog["tools"][tool_id] = self.catalog["tools"].pop("fixture")
        self.catalog["agents"]["testforge"]["tools"] = [tool_id]
        self.catalog["agents"]["testforge"]["workflows"] = {}
        operation = self.catalog["operations"]["inspect"]
        operation["tool"] = tool_id
        self.catalog["operations"] = {operation_id: operation}
        self.write_catalog()
        self.make_tool(body).replace(self.bin / tool_id)

    def test_bandit_parse_errors_and_unscanned_inputs_are_incomplete(self):
        good = {"errors": [], "metrics": {"app.py": {"loc": 1}, "_totals": {"loc": 1}}, "results": []}
        for changes, gate in [({}, "pass"), ({"errors": [{"reason": "syntax error"}]}, "incomplete"),
                              ({"metrics": {"_totals": {"loc": 0}}}, "incomplete"),
                              ({"metrics": {"_totals": {"loc": 1}}}, "incomplete")]:
            with self.subTest(changes=changes):
                value = {**good, **changes}
                self.qualified_tool("bandit-scan", "bandit", "print(" + repr(json.dumps(value)) + ")\n")
                result = self.request(operation="bandit-scan")
                self.assertEqual(result["gate"], gate)
                self.assertIn("qualification", result["checks"][0])
        self.qualified_tool("bandit-scan", "bandit", "print('not valid JSON')\n")
        self.assertEqual(self.request(operation="bandit-scan")["gate"], "incomplete")

    def test_vitest_requires_executed_tests_and_no_runtime_errors(self):
        for passed, failed, pending, errors, gate in [(1, 0, 0, 0, "pass"), (0, 0, 0, 0, "incomplete"),
                                                     (0, 0, 2, 0, "incomplete"), (1, 0, 0, 1, "incomplete"),
                                                     (0, 1, 0, 0, "fail")]:
            with self.subTest(passed=passed, failed=failed, pending=pending, errors=errors):
                value = {"numTotalTests": passed + failed + pending, "numPassedTests": passed,
                         "numFailedTests": failed, "numPendingTests": pending, "numRuntimeErrorTestSuites": errors}
                body = "import pathlib,sys\npathlib.Path(sys.argv[-1], 'vitest.json').write_text(" + repr(json.dumps(value)) + ")\nraise SystemExit(" + str(bool(failed) * 1) + ")\n"
                self.qualified_tool("vitest-test", "vitest", body)
                result = self.request(operation="vitest-test")
                self.assertEqual(result["gate"], gate)
                self.assertIn("qualification", result["checks"][0])

    def test_pytest_requires_executed_tests_and_no_collection_errors(self):
        for case, gate in [("", "pass"), ("<skipped/>", "incomplete"), ("<error/>", "incomplete"),
                           ("<failure/>", "fail")]:
            with self.subTest(case=case):
                report = '<testsuites><testsuite><testcase name="example">' + case + '</testcase></testsuite></testsuites>'
                body = "import pathlib,sys\npathlib.Path(sys.argv[-1], 'pytest.xml').write_text(" + repr(report) + ")\n"
                self.qualified_tool("pytest-test", "pytest", body)
                self.assertEqual(self.request(operation="pytest-test")["gate"], gate)
        for report in ("<testsuites/>", '<!DOCTYPE x [<!ENTITY e "text">]><testsuite><testcase name="a"/></testsuite>'):
            self.qualified_tool("pytest-test", "pytest", "import pathlib,sys\npathlib.Path(sys.argv[-1], 'pytest.xml').write_text(" + repr(report) + ")\n")
            self.assertEqual(self.request(operation="pytest-test")["gate"], "incomplete")

    def test_playwright_requires_executed_tests_and_no_global_errors(self):
        for expected, skipped, unexpected, errors, gate in [(1, 0, 0, [], "pass"), (0, 2, 0, [], "incomplete"),
                                                           (0, 0, 0, [], "incomplete"), (0, 0, 1, [], "fail"),
                                                           (1, 0, 0, [{}], "incomplete")]:
            with self.subTest(expected=expected, skipped=skipped, unexpected=unexpected, errors=errors):
                value = {"stats": {"expected": expected, "skipped": skipped, "unexpected": unexpected, "flaky": 0}, "errors": errors}
                self.qualified_tool("playwright-test", "playwright", "print(" + repr(json.dumps(value)) + ")\n")
                self.assertEqual(self.request(operation="playwright-test")["gate"], gate)

    def test_trivy_requires_checked_configuration_and_distinguishes_failures(self):
        for report, code, gate in [
            ({"Results": [{"MisconfSummary": {"Successes": 2, "Failures": 0, "Exceptions": 0}, "Misconfigurations": [{"Status": "PASS"}]}]}, 0, "pass"),
            ({"Results": [{"MisconfSummary": {"Successes": 2, "Failures": 1, "Exceptions": 0}, "Misconfigurations": [{"Status": "FAIL"}]}]}, 1, "fail"),
            ({"SchemaVersion": 2}, 0, "incomplete"),
            ({"Results": [{"MisconfSummary": {"Successes": 0, "Failures": 0, "Exceptions": 3}}]}, 0, "incomplete"),
            ({"Results": [{"MisconfSummary": {"Successes": 2, "Failures": 0, "Exceptions": 0}}]}, 1, "incomplete"),
            (None, 1, "incomplete"),
        ]:
            with self.subTest(report=report, code=code):
                body = "print(" + repr(json.dumps(report) if report is not None else "") + ")\nraise SystemExit(" + str(code) + ")\n"
                self.qualified_tool("trivy-config", "trivy", body)
                self.assertEqual(self.request(operation="trivy-config")["gate"], gate)

    def test_native_artifact_empty_directory_flood_is_bounded(self):
        self.make_tool("import pathlib,sys\np=pathlib.Path(sys.argv[-1])\nfor i in range(1030): (p/str(i)).mkdir()\n")
        result = self.request()
        self.assertEqual(result["gate"], "incomplete")
        self.assertEqual(result["checks"][0]["status"], "artifact_limit")

    def test_installed_manifest_reports_only_bounded_provenance_fields(self):
        (self.root / "installed.json").write_text(json.dumps({"family": "test", "tools": {"fixture": {"version": "1.0", "package": "fixture", "extra": "x" * 100000}}}))
        result = self.request()
        self.assertEqual(result["installed_manifest"]["tools"]["fixture"]["version"], "1.0")
        self.assertLess(len(json.dumps(result["installed_manifest"])), 4096)

    def test_report_storage_symlinks_are_refused(self):
        (self.workspace / ".specialists").symlink_to(self.root)
        with self.assertRaises(ValueError):
            self.request()

    def test_subprocess_output_is_bounded_per_stream(self):
        result = run_process([sys.executable, "-c", "import sys; print('x'*10000); print('y'*10000,file=sys.stderr)"], self.project, 3, home=self.root / "home", max_output_bytes=512)
        self.assertEqual(result.status, "output_limit")
        self.assertLessEqual(len(result.stdout), 512)
        self.assertLessEqual(len(result.stderr), 512)

    def test_child_file_limit_allows_build_outputs_above_download_limit(self):
        result = run_process([sys.executable, "-c", "import json,resource; print(json.dumps(resource.getrlimit(resource.RLIMIT_FSIZE)))"], self.project, 3, home=self.root / "home")
        self.assertEqual(json.loads(result.stdout), [268435456, 268435456])

    def test_timeout_kills_child_process_group(self):
        marker = self.project / "child-finished"
        descendant = "import pathlib,time; time.sleep(0.6); pathlib.Path(" + repr(str(marker)) + ").touch()"
        code = "import subprocess,time; subprocess.Popen([" + repr(sys.executable) + ",'-c'," + repr(descendant) + "]); time.sleep(5)"
        result = run_process([sys.executable, "-c", code], self.project, 0.15, home=self.root / "home")
        self.assertEqual(result.status, "timeout")
        time.sleep(0.7)
        self.assertFalse(marker.exists())

    def test_sigterm_reaps_child_before_cancelled_result(self):
        started = self.project / "started"
        marker = self.project / "orphan"
        code = "import pathlib,time; pathlib.Path(" + repr(str(started)) + ").touch(); time.sleep(0.6); pathlib.Path(" + repr(str(marker)) + ").touch()"
        wrapper = self.root / "wrapper.py"
        wrapper.write_text("import sys\nfrom pathlib import Path\nsys.path.insert(0," + repr(str(REPOSITORY / "deploy/agents")) + ")\nfrom boxoro_agents.process import run_process\nr=run_process([sys.executable,'-c'," + repr(code) + "],Path(" + repr(str(self.project)) + "),5,home=Path(" + repr(str(self.root / "home")) + "))\nprint(r.status)\n")
        parent = subprocess.Popen([sys.executable, str(wrapper)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(lambda: parent.poll() is None and parent.kill())
        deadline = time.monotonic() + 3
        while not started.exists() and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertTrue(started.exists())
        parent.send_signal(signal.SIGTERM)
        stdout, stderr = parent.communicate(timeout=3)
        self.assertEqual(stdout.strip(), "cancelled", stderr)
        time.sleep(0.7)
        self.assertFalse(marker.exists())


class SpecialistBuiltinTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(run_analysis, "local specialist analyses are not implemented")
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.workspace = Path(self.tmp.name)

    def write(self, name, value):
        path = self.workspace / name
        path.write_text(value)
        return str(path)

    def test_json_syntax_check_rejects_duplicate_keys(self):
        path = self.write("input.json", '{"a":1,"a":2}')
        result = run_analysis("json-check", [path], workspace=self.workspace)
        self.assertEqual(result["gate"], "fail")
        self.write("input.json", '{"a":1}')
        self.assertEqual(run_analysis("json-check", [path], workspace=self.workspace)["gate"], "pass")

    def test_csv_profile_reports_missing_values_duplicates_and_numeric_summary(self):
        path = self.write("data.csv", "name,amount\na,1\nb,3\nb,3\nc,\n")
        result = run_analysis("csv-profile", [path], workspace=self.workspace)
        self.assertEqual(result["data"]["rows"], 4)
        self.assertEqual(result["data"]["duplicate_rows"], 1)
        self.assertEqual(result["data"]["columns"]["amount"]["missing"], 1)
        self.assertAlmostEqual(result["data"]["columns"]["amount"]["numeric"]["mean"], 7 / 3)

    def test_csv_profile_refuses_duplicate_header_names(self):
        path = self.write("data.csv", "name,name\na,b\n")
        with self.assertRaises(ValueError):
            run_analysis("csv-profile", [path], workspace=self.workspace)

    def test_locale_check_finds_missing_keys_and_changed_placeholders(self):
        source = self.write("en.json", json.dumps({"welcome": "Hello {name}", "farewell": "Bye"}))
        translated = self.write("fr.json", json.dumps({"welcome": "Bonjour {nom}"}))
        result = run_analysis("locale-check", [source, translated], workspace=self.workspace)
        self.assertEqual(result["gate"], "fail")
        self.assertEqual(result["data"]["missing_keys"], ["/farewell"])
        self.assertEqual(result["data"]["placeholder_mismatches"][0]["key"], "/welcome")

    def test_rag_metrics_use_judgments_and_do_not_inflate_duplicate_hits(self):
        path = self.write("retrieval.json", json.dumps({"k": 3, "queries": [{"id": "q1", "relevant": ["a", "b"], "retrieved": ["c", "a", "b"]}]}))
        result = run_analysis("rag-evaluate", [path], workspace=self.workspace)
        metrics = result["data"]["queries"][0]
        self.assertAlmostEqual(metrics["precision_at_k"], 2 / 3)
        self.assertEqual(metrics["recall_at_k"], 1)
        self.assertEqual(metrics["reciprocal_rank"], 0.5)
        self.assertAlmostEqual(metrics["ndcg_at_k"], 0.6934264036172708)
        self.write("retrieval.json", json.dumps({"queries": [{"id": "q1", "relevant": ["a"], "retrieved": ["a", "a"]}]}))
        with self.assertRaises(ValueError):
            run_analysis("rag-evaluate", [path], workspace=self.workspace)

    def test_privacy_scan_reports_categories_without_raw_values(self):
        self.write("sample.txt", "Contact alice@example.com using 192.0.2.1.\n")
        result = run_analysis("privacy-scan", [str(self.workspace)], workspace=self.workspace)
        self.assertEqual(result["gate"], "fail")
        self.assertIn("email", result["data"]["counts"])
        self.assertNotIn("alice@example.com", json.dumps(result))
        self.assertNotIn("192.0.2.1", json.dumps(result))

    def test_inventory_reports_skipped_symlinks_as_incomplete(self):
        self.write("plain.txt", "hello")
        (self.workspace / "link").symlink_to("/etc/passwd")
        result = run_analysis("inventory", [str(self.workspace)], workspace=self.workspace)
        self.assertEqual(result["gate"], "incomplete")
        self.assertEqual(result["data"]["files"][0]["sha256"], "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        self.assertEqual(len(result["data"]["files"]), 1)

    def test_knowledge_audit_identifies_broken_local_markdown_links(self):
        self.write("README.md", "# Intro\n\n[Good](guide.md) [Missing](absent.md)\n")
        self.write("guide.md", "# Guide\n")
        result = run_analysis("knowledge-audit", [str(self.workspace)], workspace=self.workspace)
        self.assertEqual(result["gate"], "fail")
        self.assertEqual(len(result["data"]["broken_links"]), 1)
        self.assertEqual(result["data"]["broken_links"][0]["target"], "absent.md")

    def test_scaffold_creates_new_package_and_refuses_overwrite(self):
        destination = self.workspace / "my-agent"
        result = run_analysis("scaffold", [str(destination), "support_agent"], workspace=self.workspace)
        self.assertEqual(result["gate"], "pass")
        self.assertTrue((destination / "pyproject.toml").is_file())
        self.assertTrue((destination / "src/support_agent/__init__.py").is_file())
        with self.assertRaises(ValueError):
            run_analysis("scaffold", [str(destination), "support_agent"], workspace=self.workspace)


if __name__ == "__main__":
    unittest.main()
