"""Opt-in contract tests against real installed specialist CLIs, never mock binaries.

Run with BOXORO_TEST_SPECIALIST_TOOLS=1. By default tools are discovered in the
adjacent specialist-tools, specialist-js-tools and specialist-node installations.
Override BOXORO_TEST_SPECIALIST_PYTHON_BIN, BOXORO_TEST_SPECIALIST_JS_BIN and
BOXORO_TEST_SPECIALIST_NODE for another qualified installation. These tests do
not install dependencies, contact accounts, or substitute for container smoke.
"""

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "deploy/agents"))

from boxoro_agents.runner import run_request  # noqa: E402


@unittest.skipUnless(os.environ.get("BOXORO_TEST_SPECIALIST_TOOLS") == "1", "real specialist CLI fixtures are opt-in")
class SpecialistRecipeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.workspace = Path(self.temporary.name)
        self.project = self.workspace / "project"
        self.project.mkdir()
        self.tool_bin = self.workspace / "bin"
        self.tool_bin.mkdir()
        self.catalog = json.loads((ROOT / "deploy/agents/catalog.json").read_text())

    def install_fixture_launcher(self, tool):
        """Adapt the Node path only; run the exact installed upstream executable."""
        python_bin = Path(os.environ.get("BOXORO_TEST_SPECIALIST_PYTHON_BIN", ROOT.parent / "specialist-tools/bin"))
        js_bin = Path(os.environ.get("BOXORO_TEST_SPECIALIST_JS_BIN", ROOT.parent / "specialist-js-tools/bin"))
        source = next((directory / tool for directory in (python_bin, js_bin) if (directory / tool).is_file()), None)
        if source is None:
            self.skipTest("installed CLI is unavailable: " + tool)
        with source.open("rb") as stream:
            prefix = stream.read(100)
        command = [str(source)]
        if prefix.startswith(b"#!") and b"node" in prefix.split(b"\n", 1)[0]:
            node = Path(os.environ.get("BOXORO_TEST_SPECIALIST_NODE", ROOT.parent / "specialist-node/bin/node"))
            if not node.is_file():
                self.skipTest("the qualified Node executable is unavailable")
            command.insert(0, str(node))
        launcher = self.tool_bin / tool
        launcher.write_text("#!" + sys.executable + "\nimport os,sys\ncommand=" + repr(command)
                            + "\nos.execv(command[0], command + sys.argv[1:])\n")
        launcher.chmod(0o755)
        return command

    def write(self, name, content):
        path = self.project / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def run_operation(self, agent, operation, *, path=None, parameters=None, **permissions):
        if operation not in self.catalog["operations"]:
            self.fail("the fixed recipe is not registered: " + operation)
        self.install_fixture_launcher(self.catalog["operations"][operation]["tool"])
        request = {"action": "run", "agent": agent, "operation": operation,
                   "path": str(path or self.project), "parameters": parameters or {},
                   "allow_project_code": True, "allow_network": True, "allow_mutation": True,
                   **permissions}
        try:
            return run_request(request, workspace=self.workspace, tool_bin=self.tool_bin,
                               catalog_path=ROOT / "deploy/agents/catalog.json")
        except ValueError as error:
            self.fail(operation + " rejected the fixture request: " + str(error))

    def output(self, result, stream="stdout"):
        return (self.workspace / result["checks"][0]["reports"][stream]).read_text()

    def assert_gate(self, result, expected):
        diagnostics = [self.output(result, name)[:3000] for name in ("stdout", "stderr")] if result["checks"][0].get("reports") else result
        self.assertEqual(result["gate"], expected, diagnostics)

    def test_madge_detects_typescript_cycle_in_a_directory(self):
        self.write("a.ts", "import { b } from './b';\nexport const a = () => b;\n")
        self.write("b.ts", "import { a } from './a';\nexport const b = () => a;\n")
        result = self.run_operation("code-architect", "madge-cycles")
        self.assert_gate(result, "fail")
        self.assertEqual(json.loads(self.output(result)), [["a.ts", "b.ts"]])
        self.write("b.ts", "export const b = 1;\n")
        self.assert_gate(self.run_operation("code-architect", "madge-cycles"), "pass")

    def test_buf_lint_classifies_real_lint_findings(self):
        self.write("buf.yaml", "version: v2\n")
        self.write("item.proto", 'syntax = "proto3";\npackage item.v1;\nmessage Item { string id = 1; }\n')
        result = self.run_operation("api-architect", "buf-lint")
        self.assert_gate(result, "fail")
        self.assertIn("PACKAGE_DIRECTORY_MATCH", self.output(result))
        self.write("item.proto", "not a valid protobuf definition\n")
        result = self.run_operation("api-architect", "buf-lint")
        self.assert_gate(result, "fail")
        self.assertIn('"type":"COMPILE"', self.output(result))
        self.write("buf.yaml", "version: [not-valid\n")
        self.assert_gate(self.run_operation("api-architect", "buf-lint"), "incomplete")

    def test_buf_check_plugin_requires_project_code_and_reports_execution_failure(self):
        marker = self.project / "plugin-ran"
        plugin = self.write("fixture-plugin", "#!" + sys.executable + "\nfrom pathlib import Path\nPath("
                            + repr(str(marker)) + ").touch()\nraise SystemExit(1)\n")
        plugin.chmod(0o755)
        self.write("buf.yaml", "version: v2\nplugins:\n  - plugin: " + str(plugin) + "\n")
        self.write("item.proto", 'syntax = "proto3";\npackage item.v1;\nmessage Item { string id = 1; }\n')
        result = self.run_operation("api-architect", "buf-lint", allow_project_code=False)
        self.assertEqual(result["checks"][0]["status"], "blocked")
        self.assertFalse(marker.exists())
        result = self.run_operation("api-architect", "buf-lint")
        self.assertTrue(marker.exists(), "the harmless fixture must exercise real plugin execution")
        self.assert_gate(result, "incomplete")

    def test_buf_breaking_compares_supplied_local_baseline(self):
        baseline = self.project / "baseline"
        candidate = self.project / "candidate"
        for directory in (baseline, candidate):
            directory.mkdir()
            (directory / "buf.yaml").write_text("version: v2\nbreaking:\n  use: [FILE]\n")
            (directory / "item.proto").write_text('syntax = "proto3";\npackage item.v1;\nmessage Item { string id = 1; }\n')
        parameters = {"baseline": str(baseline), "candidate": str(candidate)}
        self.assert_gate(self.run_operation("api-architect", "buf-breaking", parameters=parameters), "pass")
        (candidate / "item.proto").write_text('syntax = "proto3";\npackage item.v1;\nmessage Item { int32 id = 1; }\n')
        result = self.run_operation("api-architect", "buf-breaking", parameters=parameters)
        self.assert_gate(result, "fail")
        self.assertIn("FIELD_SAME_TYPE", self.output(result))
        (candidate / "item.proto").write_text("not a valid protobuf definition\n")
        self.assert_gate(self.run_operation("api-architect", "buf-breaking", parameters=parameters), "incomplete")

    def test_sacrebleu_uses_explicit_chinese_tokenizer(self):
        reference = self.write("reference.txt", "这是一个用于测试的中文句子。\n")
        candidate = self.write("candidate.txt", reference.read_text())
        result = self.run_operation("localeqa", "sacrebleu-score", parameters={
            "reference": str(reference), "candidate": str(candidate), "tokenizer": "zh"})
        self.assert_gate(result, "pass")
        scores = json.loads(self.output(result))
        self.assertEqual(scores[0]["score"], 100)
        self.assertIn("tok:zh", scores[0]["signature"])

    def test_markdownlint_file_checks_the_requested_file(self):
        document = self.write("README.md", "#Title without a space\n")
        result = self.run_operation("docsmith", "markdownlint-file", parameters={"input": str(document)})
        self.assert_gate(result, "fail")
        self.assertIn("MD018", self.output(result, "stderr"))
        self.write("README.md", "# Valid heading\n")
        self.assert_gate(self.run_operation("docsmith", "markdownlint-file", parameters={"input": str(document)}), "pass")

    def test_markdownlint_configured_fixes_require_mutation_and_ignore_extra_globs(self):
        document = self.write("README.md", "#Title without a space\n")
        extra = self.write("extra.md", "#Another missing space\n")
        self.write(".markdownlint-cli2.jsonc", '{"fix":true,"globs":["extra.md"]}')
        parameters = {"input": str(document)}
        result = self.run_operation("docsmith", "markdownlint-file", parameters=parameters, allow_mutation=False)
        self.assertEqual(result["checks"][0]["status"], "blocked")
        self.assertEqual(document.read_text(), "#Title without a space\n")
        self.assert_gate(self.run_operation("docsmith", "markdownlint-file", parameters=parameters), "pass")
        self.assertEqual(document.read_text(), "# Title without a space\n")
        self.assertEqual(extra.read_text(), "#Another missing space\n")

    def test_markdownlint_bracketed_file_is_literal_and_does_not_select_sibling(self):
        selected = self.write("README[1].md", "#Selected heading\n")
        sibling = self.write("README1.md", "#Unselected heading\n")
        self.write(".markdownlint-cli2.jsonc", '{"fix":true}')
        result = self.run_operation("docsmith", "markdownlint-file", parameters={"input": str(selected)})
        self.assert_gate(result, "pass")
        self.assertEqual(selected.read_text(), "# Selected heading\n")
        self.assertEqual(sibling.read_text(), "#Unselected heading\n")
        self.assertIn("Linting: 1 file\n", self.output(result))

    def test_markdownlint_bracketed_directory_is_selected_as_working_directory(self):
        selected = self.write("docs[1]/README.md", "#Selected heading\n")
        sibling = self.write("docs1/README.md", "#Unselected heading\n")
        self.write("docs[1]/.markdownlint-cli2.jsonc", '{"fix":true}')
        self.write("docs1/.markdownlint-cli2.jsonc", '{"fix":true}')
        result = self.run_operation("docsmith", "markdownlint-check", path=selected.parent)
        self.assert_gate(result, "pass")
        self.assertEqual(selected.read_text(), "# Selected heading\n")
        self.assertEqual(sibling.read_text(), "#Unselected heading\n")
        self.assertIn("Linting: 1 file\n", self.output(result))

    def test_markdownlint_recursive_rejects_file_target_before_execution(self):
        self.install_fixture_launcher("markdownlint")
        selected = self.write("README.md", "#Selected heading\n")
        self.write(".markdownlint-cli2.jsonc", '{"fix":true}')
        with self.assertRaisesRegex(ValueError, "markdownlint-check requires a directory target"):
            run_request({"action": "run", "agent": "docsmith", "operation": "markdownlint-check",
                         "path": str(selected), "allow_project_code": True, "allow_mutation": True},
                        workspace=self.workspace, tool_bin=self.tool_bin,
                        catalog_path=ROOT / "deploy/agents/catalog.json")
        self.assertEqual(selected.read_text(), "#Selected heading\n")
        empty = self.project / "empty"
        empty.mkdir()
        result = self.run_operation("docsmith", "markdownlint-check", path=empty)
        self.assert_gate(result, "incomplete")
        self.assertIn("Linting: 0 files", self.output(result))

    def test_redocly_bundle_resolves_local_references_without_forcing_invalid_output(self):
        self.write("schema.yaml", "type: object\nproperties:\n  id:\n    type: string\n")
        spec = {"openapi": "3.0.3", "info": {"title": "Fixture", "version": "1.0.0"}, "paths": {},
                "components": {"schemas": {"Item": {"$ref": "./schema.yaml"}}}}
        path = self.write("api.json", json.dumps(spec))
        result = self.run_operation("api-architect", "redocly-bundle", parameters={"input": str(path)})
        self.assert_gate(result, "pass")
        artifacts = result["checks"][0]["artifacts"]
        bundle = json.loads((self.workspace / next(a["path"] for a in artifacts if a["path"].endswith("openapi.bundle.json"))).read_text())
        schemas = bundle["components"]["schemas"]
        self.assertTrue(any(s.get("properties", {}).get("id", {}).get("type") == "string" for s in schemas.values()))
        spec["components"]["schemas"]["Missing"] = {"$ref": "./missing.yaml"}
        self.write("api.json", json.dumps(spec))
        result = self.run_operation("api-architect", "redocly-bundle", parameters={"input": str(path)})
        self.assert_gate(result, "fail")
        self.assertEqual(result["checks"][0]["artifacts"], [])

    def prepare_i18next(self):
        self.write("src/app.ts", "t('greeting');\n")
        self.write("i18next.config.mjs", "export default {locales: ['en', 'de'], extract: {input: ['src/**/*.ts'], output: 'locales/{{language}}/{{namespace}}.json'}};\n")
        self.write("locales/en/translation.json", '{"greeting":"Hello"}')
        self.write("locales/de/translation.json", '{"greeting":"Hallo"}')

    def test_i18next_missing_translation_is_a_finding(self):
        self.prepare_i18next()
        self.assert_gate(self.run_operation("localeqa", "i18next-status"), "pass")
        self.write("locales/de/translation.json", "{}")
        result = self.run_operation("localeqa", "i18next-status")
        self.assert_gate(result, "fail")
        self.assertIn("1 absent", self.output(result))

    def test_i18next_unused_report_does_not_remove_messages(self):
        self.prepare_i18next()
        self.assert_gate(self.run_operation("localeqa", "i18next-unused"), "pass")
        source = self.write("locales/en/translation.json", '{"greeting":"Hello","retired":"Old key"}')
        original = source.read_bytes()
        result = self.run_operation("localeqa", "i18next-unused")
        self.assert_gate(result, "fail")
        self.assertIn("retired", self.output(result))
        self.assertEqual(source.read_bytes(), original)

    def test_i18next_invalid_config_does_not_count_as_a_translation_finding(self):
        self.prepare_i18next()
        self.write("i18next.config.mjs", "export default { this is invalid;\n")
        for operation in ("i18next-status", "i18next-unused"):
            with self.subTest(operation=operation):
                self.assert_gate(self.run_operation("localeqa", operation), "incomplete")

    def test_i18next_unused_rejects_incomplete_source_parsing(self):
        self.prepare_i18next()
        self.write("src/app.ts", "const broken = ;\nt('greeting');\n")
        result = self.run_operation("localeqa", "i18next-unused")
        self.assert_gate(result, "incomplete")
        self.assertIn("Some source files could not be parsed", self.output(result) + self.output(result, "stderr"))

    def test_i18next_status_rejects_source_or_plugin_gaps_when_other_keys_are_found(self):
        self.prepare_i18next()
        self.write("src/broken.ts", "const broken = ;\n")
        result = self.run_operation("localeqa", "i18next-status")
        self.assert_gate(result, "incomplete")
        self.assertIn("Keys Found:", self.output(result))
        self.assertIn("Skipping file", self.output(result, "stderr"))
        self.write("src/broken.ts", "const valid = 1;\n")
        self.write("i18next.config.mjs", "export default {locales: ['en', 'de'], extract: {input: ['src/**/*.ts'], output: 'locales/{{language}}/{{namespace}}.json'}, plugins: [{name: 'broken-fixture', onVisitNode() {throw new Error('fixture failure')}}]};\n")
        result = self.run_operation("localeqa", "i18next-status")
        self.assert_gate(result, "incomplete")
        self.assertIn("Plugin broken-fixture onVisitNode failed", self.output(result, "stderr"))

    def test_duckdb_columns_returns_types_and_nullability_without_changing_database(self):
        command = self.install_fixture_launcher("duckdb")
        database = self.project / "inventory.duckdb"
        subprocess.run([*command, str(database), "-c", "CREATE TABLE orders(id BIGINT PRIMARY KEY, amount DECIMAL(12,2) NOT NULL, memo VARCHAR);"],
                       cwd=self.project, check=True, capture_output=True, timeout=30)
        digest = hashlib.sha256(database.read_bytes()).hexdigest()
        result = self.run_operation("database-pro", "duckdb-columns", parameters={"input": str(database)})
        self.assert_gate(result, "pass")
        rows = json.loads(self.output(result))
        self.assertEqual([(r["column_name"], r["data_type"], r["is_nullable"]) for r in rows],
                         [("id", "BIGINT", "NO"), ("amount", "DECIMAL(12,2)", "NO"), ("memo", "VARCHAR", "YES")])
        self.assertEqual(hashlib.sha256(database.read_bytes()).hexdigest(), digest)

    def test_dlt_inventory_uses_supplied_directory_instead_of_empty_runner_home(self):
        pipelines = self.project / "pipelines"
        (pipelines / "fixture_pipeline").mkdir(parents=True)
        result = self.run_operation("dataworks", "dlt-list-pipelines", path=pipelines)
        self.assert_gate(result, "pass")
        self.assertIn("fixture_pipeline", self.output(result))

    def test_failed_profiler_workloads_are_incomplete_and_preserve_existing_evidence(self):
        script = self.write("failed_workload.py", "values = [bytearray(10000) for _ in range(10)]\nraise RuntimeError('fixture workload failed')\n")
        result = self.run_operation("performance-lab", "memray-profile", parameters={"script": str(script)})
        self.assert_gate(result, "incomplete")
        self.assertTrue(any(a["path"].endswith("memray.bin") for a in result["checks"][0]["artifacts"]))
        self.assertIn("fixture workload failed", self.output(result, "stderr"))
        missing_dependency = self.write("missing_dependency.py", "import missing_specialist_fixture_dependency\n")
        result = self.run_operation("performance-lab", "pyinstrument-profile", parameters={"script": str(missing_dependency)})
        self.assert_gate(result, "incomplete")
        self.assertIn("ModuleNotFoundError", self.output(result, "stderr"))


if __name__ == "__main__":
    unittest.main()
