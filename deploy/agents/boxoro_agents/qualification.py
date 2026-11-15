"""Evidence checks for native tools whose exit status alone is insufficient.

This registry is image-owned code, never a project-provided validator. Report
parsing is bounded and cannot import plugins, resolve URLs or execute code.
"""

import hashlib
import os
import stat
import xml.etree.ElementTree as ET

from .catalog import strict_json
from .reports import open_directory

MAX_REPORT = 8 * 1024 * 1024


def count(value, name):
    result = value.get(name)
    if type(result) is not int or result < 0:
        raise ValueError("The native report lacks a valid " + name + " count.")
    return result


def json_object(data):
    if len(data) > MAX_REPORT:
        raise ValueError("The native report exceeds the 8 MiB qualification limit.")
    value = strict_json(data)
    if not isinstance(value, dict):
        raise ValueError("The native report must be a JSON object.")
    return value


def native_report(check, directory, filename, workspace):
    """Read one exact already-inventoried report, without following links."""
    relative = str((directory / filename).relative_to(workspace))
    record = next((item for item in check["artifacts"] if item["path"] == relative), None)
    if record is None:
        raise ValueError("The command did not produce the required " + filename + " evidence.")
    parent = open_directory(directory)
    descriptor = None
    try:
        descriptor = os.open(filename, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_uid != os.geteuid() or info.st_size > MAX_REPORT:
            raise ValueError("The native report is not a bounded owned regular file.")
        with os.fdopen(descriptor, "rb") as stream:
            descriptor = None
            data = stream.read(MAX_REPORT + 1)
        if len(data) != record["bytes"] or hashlib.sha256(data).hexdigest() != record["sha256"]:
            raise ValueError("The native report changed after artifact collection.")
        return data
    finally:
        if descriptor is not None:
            os.close(descriptor)
        os.close(parent)


def bandit(check, process, artifacts, workspace):
    value = json_object(process.stdout)
    if not isinstance(value.get("errors"), list) or not isinstance(value.get("results"), list):
        raise ValueError("Bandit did not produce its expected errors and results arrays.")
    metrics = value.get("metrics")
    if not isinstance(metrics, dict) or not isinstance(metrics.get("_totals"), dict):
        raise ValueError("Bandit did not report scan coverage metrics.")
    lines = count(metrics["_totals"], "loc")
    files = sum(isinstance(item, dict) for name, item in metrics.items() if name != "_totals")
    check["qualification"].update(scanned_files=files, scanned_lines=lines, scan_errors=len(value["errors"]), findings=len(value["results"]))
    if value["errors"]:
        raise ValueError("Bandit reported parse or scan errors; coverage is incomplete even when its exit code is zero.")
    if not files or not lines:
        raise ValueError("Bandit did not scan any Python source lines.")
    if value["results"] and check["status"] == "passed":
        check["status"] = "findings"


def vitest(check, process, artifacts, workspace):
    value = json_object(native_report(check, artifacts, "vitest.json", workspace))
    passed = count(value, "numPassedTests")
    failed = count(value, "numFailedTests")
    total = count(value, "numTotalTests")
    errors = count(value, "numRuntimeErrorTestSuites") if "numRuntimeErrorTestSuites" in value else 0
    failed_suites = count(value, "numFailedTestSuites") if "numFailedTestSuites" in value else 0
    check["qualification"].update(executed_tests=passed + failed, total_tests=total, runtime_errors=errors)
    if errors or (failed_suites and not failed) or (value.get("success") is False and not failed):
        raise ValueError("Vitest reported runtime or suite setup errors; test coverage is incomplete.")
    if not passed + failed or passed + failed > total:
        raise ValueError("Vitest did not report a valid positive count of executed tests.")
    if failed and check["status"] == "passed":
        check["status"] = "findings"


def pytest(check, process, artifacts, workspace):
    data = native_report(check, artifacts, "pytest.xml", workspace)
    # Native pytest JUnit never needs DTDs or entity declarations. Reject them
    # before parsing, including documents with expansion payloads.
    try:
        text = data.decode("utf-8-sig")
    except UnicodeError as error:
        raise ValueError("The pytest JUnit report must use UTF-8.") from error
    if "\0" in text or "<!doctype" in text.lower() or "<!entity" in text.lower():
        raise ValueError("JUnit evidence cannot contain DTD or entity declarations.")
    try:
        value = ET.fromstring(text)
    except ET.ParseError as error:
        raise ValueError("The pytest JUnit report is not valid XML.") from error
    if value.tag not in {"testsuite", "testsuites"}:
        raise ValueError("The pytest report is not a JUnit test suite.")
    cases = list(value.iter("testcase"))
    skipped = sum(case.find("skipped") is not None for case in cases)
    errors = sum(case.find("error") is not None for case in cases)
    failed = sum(case.find("failure") is not None for case in cases)
    executed = len(cases) - skipped - errors
    check["qualification"].update(executed_tests=executed, total_tests=len(cases), skipped_tests=skipped, setup_errors=errors)
    if errors:
        raise ValueError("pytest reported collection or setup errors; coverage is incomplete.")
    if executed <= 0:
        raise ValueError("pytest did not report any executed tests.")
    if failed and check["status"] == "passed":
        check["status"] = "findings"


def playwright(check, process, artifacts, workspace):
    value = json_object(process.stdout)
    stats = value.get("stats")
    if not isinstance(stats, dict) or not isinstance(value.get("errors"), list):
        raise ValueError("Playwright did not report test statistics and global errors.")
    expected, unexpected, flaky, skipped = [count(stats, key) for key in ("expected", "unexpected", "flaky", "skipped")]
    executed = expected + unexpected + flaky
    check["qualification"].update(executed_tests=executed, skipped_tests=skipped, global_errors=len(value["errors"]))
    if value["errors"]:
        raise ValueError("Playwright reported global or suite setup errors; coverage is incomplete.")
    if not executed:
        raise ValueError("Playwright did not report any executed tests.")
    if unexpected and check["status"] == "passed":
        check["status"] = "findings"


def trivy_config(check, process, artifacts, workspace):
    value = json_object(process.stdout)
    results = value.get("Results")
    if not isinstance(results, list) or not results:
        raise ValueError("Trivy did not report any checked infrastructure configuration.")
    successes = failures = ignored = 0
    for result in results:
        summary = result.get("MisconfSummary") if isinstance(result, dict) else None
        if not isinstance(summary, dict):
            raise ValueError("Trivy omitted configuration check coverage counts.")
        successes += count(summary, "Successes")
        failures += count(summary, "Failures")
        ignored += count(summary, "Exceptions") if "Exceptions" in summary else 0
    check["qualification"].update(passed_checks=successes, failed_checks=failures, ignored_checks=ignored)
    if not successes + failures:
        raise ValueError("Trivy did not complete any non-ignored configuration checks.")
    if value.get("Errors") or (check["status"] == "findings" and not failures):
        raise ValueError("Trivy's nonzero exit is not supported by configuration findings; coverage is incomplete.")
    # --include-non-failures also emits PASS entries in Misconfigurations. Its
    # array length is not a failure count; use the scanner's explicit summary.
    if failures:
        check["status"] = "findings"


VALIDATORS = {
    ("bandit-scan", "bandit"): bandit,
    ("vitest-test", "vitest"): vitest,
    ("pytest-test", "pytest"): pytest,
    ("playwright-test", "playwright"): playwright,
    ("trivy-config", "trivy"): trivy_config,
}


def qualify_result(check, process, artifacts, workspace):
    validator = VALIDATORS.get((check.get("operation"), check["tool"]))
    if validator is None or check["status"] not in {"passed", "findings"}:
        return
    check["qualification"] = {"validator": check["operation"]}
    try:
        validator(check, process, artifacts, workspace)
    except (ValueError, OSError) as error:
        check["original_status"] = check["status"]
        check["status"] = "error"
        check["reason"] = "Native evidence qualification failed: " + str(error)[:512]
