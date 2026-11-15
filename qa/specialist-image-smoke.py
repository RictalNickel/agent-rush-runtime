"""Real container qualification; called by specialist-image-smoke.sh only."""
import errno
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

assert os.getuid() == 1000, "unexpected image user"
family = sys.argv[1]
try:
    Path("/opt/box/agents/must-not-write").write_text("x")
except OSError as error:
    assert error.errno in (errno.EACCES, errno.EROFS), error
else:
    raise AssertionError("installation is writable")

if family == "finance-service":
    # Import the real extension set and execute deterministic local analytics.
    # Network remains disabled for the entire container.
    os.environ["HOME"] = "/var/lib/boxoro-finance"
    os.environ["XDG_CACHE_HOME"] = "/var/lib/boxoro-finance/cache"
    os.environ["OPENBB_AUTO_BUILD"] = "false"
    sys.path.insert(0, "/opt/finance")
    from boxoro_finance.service import Settings, prepare_state
    prepare_state(Settings(token="offline-image-smoke-" + "x" * 32))
    from boxoro_finance.openbb_bridge import execute_tool
    status = execute_tool("openbb_status", {})
    result = execute_tool("openbb_query", {
        "command": "quantitative.summary", "parameters": {},
        "body": {"target": "close", "data": [{"close": n} for n in (1, 2, 3, 4)]},
    })
    assert result["gate"] == "pass" and result["data"]["results"]["mean"] == 2.5, result
    assert result["data"]["extra"]["metadata"]["route"] == "/quantitative/summary", result
    print(json.dumps({"smoke": "passed", "family": family, "status": status,
                      "offline_analytics": result}, allow_nan=False))
    raise SystemExit(0)

sys.path.insert(0, "/opt/box/agents")
from boxoro_agents.runner import run_request  # noqa: E402 - immutable image modules
from boxoro_agents.process import clean_environment  # noqa: E402
spec = importlib.util.spec_from_file_location("installer", "/opt/box/agents/install.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)
catalog = json.loads(Path("/opt/box/agents/catalog.json").read_text())
manifest = json.loads(Path("/opt/box/agents/installed.json").read_text())
selected = installer.select_tools(catalog, family, None, False)
# Boxoro executes ordinary commands with bash -lc. Exercise profile ordering as
# well as the runner's deliberately independent clean environment.
login = subprocess.run(["/bin/bash", "-lc", "command -v node; command -v boxoro-analysis"],
                       capture_output=True, text=True, check=True, timeout=15)
assert login.stdout.splitlines() == ["/opt/box/node/bin/node", "/opt/box/agents/bin/boxoro-analysis"], login.stdout
if Path("/opt/security/bin/gitleaks").exists():
    login = subprocess.run(["/bin/bash", "-lc", "case :$PATH: in *:/opt/security/bin:*) exit 0;; *) exit 1;; esac"],
                           capture_output=True, text=True, check=True, timeout=15)
home = Path("/home/agent/probes")
for directory in (home, home / "tmp", home / "cache", home / "config"):
    directory.mkdir(mode=0o700, exist_ok=True)
environment = clean_environment(home, Path("/workspace"))
probes = []
for name in selected:
    assert name in manifest["tools"], "installation manifest omitted " + name
    tool = catalog["tools"][name]
    command = ["/opt/box/agents/bin/" + name, *tool.get("probe", ["--version"])]
    completed = subprocess.run(command, env=environment, cwd="/workspace",
                               capture_output=True, text=True, timeout=90)
    assert completed.returncode in tool.get("probe_success_codes", [0]), (
        name, completed.returncode, completed.stdout[-2000:], completed.stderr[-2000:])
    probes.append(name)

fixture = Path("/workspace/smoke")
fixture.mkdir()
(fixture / "data.csv").write_text("value\n1\n2\n3\n")
profile = run_request({"action": "run", "agent": "dataworks", "operation": "csv-profile",
                       "path": "smoke", "parameters": {"input": "data.csv"}})
assert profile["gate"] == "pass", profile
evidence = json.loads((Path("/workspace") / profile["checks"][0]["reports"]["stdout"]).read_text())
assert evidence["gate"] == "pass", evidence

# A wrong tool or missing optional installation must never become a clean result.
assert len(run_request({"action": "list"})["agents"]) == 21
if "mypy" in selected:
    (fixture / "app.py").write_text("number: int = 1\n")
    checked = run_request({"action": "run", "agent": "code-architect", "operation": "mypy-check",
                           "path": "smoke", "allow_project_code": True})
    assert checked["gate"] == "pass", checked
    assert not (fixture / ".mypy_cache").exists(), "mypy wrote a project cache"
if family in ("browser", "suite"):
    assert Path("/opt/box/chromium").is_file(), "browser executable missing"
    # Exercise a real browser launch against a local file without external I/O.
    (fixture / "page.html").write_text("<!doctype html><html lang=en><title>Smoke</title><h1>Boxoro</h1></html>")
    program = "from playwright.sync_api import sync_playwright\nwith sync_playwright() as p:\n b=p.chromium.launch(); page=b.new_page(); page.goto('file:///workspace/smoke/page.html'); assert page.title()=='Smoke'; b.close()"
    subprocess.run(["/opt/box/venv/bin/python", "-I", "-c", program], env=environment, check=True, timeout=60)
if family in ("finance", "suite"):
    subprocess.run(["/opt/finance/venv/bin/python", "-I", "-c",
                    "import numpy,pandas,scipy,statsmodels,QuantLib,cvxpy; print(QuantLib.__version__)"],
                   env=environment, check=True, timeout=60)
print(json.dumps({"smoke": "passed", "family": family, "probed_tools": probes,
                  "uid": os.getuid(), "read_only_root": True, "network": "none",
                  "builtin_csv": profile["gate"]}, allow_nan=False))
