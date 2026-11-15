# Specialist implementation validation

Validation date: **2026-11-15**. This records executed checks for the specialist-agent change based on the existing security branch. The source, locks, catalog, operating guides and qualification scripts are delivered together. A successful source-level or CLI check is not a claim that a production worker image or external account has been deployed.

This is the **initial implementation validation** retained with its original counts. The subsequent [verification and efficiency review](SPECIALIST_REVIEW.md) records the current 156-operation catalog, additional regression/real-tool checks, measured improvements and the follow-up GitHub change.

## Verified inventory

| Item | Observed result |
| --- | --- |
| Catalog | 21 profiles, 167 distinct tools, 151 registered operations, 10 connector records. |
| Tool availability | 118 default image entries, 34 optional, 11 host/service prerequisites, four external entries. |
| Native skills | 21 new specialist skills plus the four existing general/security built-ins. |
| Dependency locks | Every default and optional installable tool has a resolved lock; referenced file hashes validate. |
| Actual package installations | All 40 initially selected Python CLI environments and 29 npm environments installed using the reviewed installer and locks; 18 native release tools installed and probed. Checkov and BackstopJS were subsequently excluded from default images after advisory review. |
| Actual CLI probes | **87/87 passed** after fixing Translate Toolkit's readiness command and applying the npm overrides. This includes two packages now classified as optional. |
| OpenBB | The actual installed all-provider environment discovered **1,138 allowed commands across 23 namespaces**, with `openbb-core==2.0.1`. |

The [machine-readable probe receipt](validation/specialist-tool-probes.json) records exact locked versions and the catalog/install-lock hashes.

Tool probes establish executable availability and version/help behavior. They do not establish that every operation succeeds on every project, that an optional model has its weights, that a database exists, or that a provider credential works.

## Automated checks

### Rust control plane and worker integration

The final native run completed **257 passing tests**, with **nine existing ignored host/live tests** and **one known environment-specific test filtered**. This includes specialist request validation, duplicate-parameter rejection, catalog membership, built-in skills, versioned owner configuration, disabled-tool behavior, safe helper dispatch, authenticated HTTP discovery and MCP metadata discovery.

```sh
cargo test --locked --tests -- --skip exec_wrapper_keeps_the_command_out_of_process_arguments
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
```

Formatting and Clippy completed successfully. The filtered test was first run and failed because this executor has no `/tmp`; it invokes the existing wrapper's hard-coded `/tmp` location. That production wrapper was not changed. Run the full unfiltered suite on a normal Linux development/CI host. The ignored tests still require their documented real host prerequisites.

### Python helpers and isolated test dependencies

The combined security/specialist/finance base suite completed **168 tests with no failures and four optional-backend skips** in the development environment. That means 164 tests ran successfully; it does not mean skipped backends were exercised there.

```sh
python3 -m unittest discover -s qa -p 'test_*.py' -v
```

A fresh minimal virtual environment installed only the hashed `qa/requirements.lock` and ran the same **168 tests without failures**, with **seven expected skips** because fewer optional backends were present. The manual CI workflow now creates this explicit environment instead of assuming jsonschema is globally installed.

The suite covers fixed-command membership and parameters, strict JSON and numeric limits, path/symlink boundaries, clean environments, exit/findings/error classification, missing tools, process cancellation/deadline cleanup, output and artifact ceilings, workflow short-circuiting, native report qualification, all eight first-party local analyses, dependency hash/path/archive checks, canonical system launchers, release-cache refresh and npm refresh/script policy.

A redacted Gitleaks scan of the complete staged patch (about 6.23 MB) found zero credential matches. This is a bounded static check, not proof that no secret could exist.

The scoped Ruff gate uses `--isolated --select E4,E7,E9,F` on the added Python source/tests. Shell scripts pass `bash -n`, JavaScript passes `node --check`, and `git diff --check` reports no whitespace errors.

### Finance environments

**50 unique Finance tests pass across their intended environments:** 43 baseline/schema/HTTP tests, six tests using actual numerical backends, and one actual isolated OpenBB SDK test. These overlap the combined Python suite and must not be added to it as unrelated test cases.

The real advanced tests use statsmodels, SciPy and QuantLib from the hashed environment. They check known regression coefficients, a random-walk forecast, constrained portfolio solutions, a par bond with calendar cash flows, risk-free convention consistency and failure handling.

The real OpenBB test runs a fresh worker with provider networking disabled. `quantitative.summary` on values 1,2,3,4 returns count 4, mean 2.5 and variance 5/3. The response retains OBBject metadata, including its actual route and command arguments. Cold workers took roughly 38–42 seconds in this environment.

```sh
# Run in the advanced numerical environment.
python -m unittest qa.test_finance_advanced -v

# Run in the all-provider OpenBB environment.
BOXORO_TEST_OPENBB=1 python -m unittest qa.test_finance_sdk -v
```

### Chat interface

The actual Rust server served the page, security headers and operator login. Playwright exercised the UI against the existing in-test session API fixture because no worker was connected. All **12 browser tests passed**. The full suite covered session send/interjection/cancel/reply flows, files, screenshots, models, downloads, desktop/mobile layouts and the specialist picker.

Chrome for Testing's full desktop binary could not start because this executor disallows its Unix process-singleton socket. The official Chrome Headless Shell 157.0.8103.78 started successfully; it was selected explicitly through `BOXORO_QA_CHROMIUM`. Screenshot review confirmed the picker and description fit the 390 px mobile layout and the desktop composer.

The final review added regression cases for a transient specialist-discovery error and Unicode session titles. The browser tests use the actual page implementation; they do not claim a live model/provider/worker test.

## Real operation fixtures

| Operation / behavior | Executed evidence |
| --- | --- |
| Bandit | Invalid Python and no-source scans return exit 0 upstream but are now classified incomplete using native errors and source counts. |
| pytest / Vitest / Playwright | Valid suites pass; empty/all-skipped or setup-error reports are incomplete. Vitest cannot inherit a project's `passWithNoTests:true` to claim coverage. |
| Trivy configuration | With an empty cache and blocked proxy egress, embedded rules reported 68 successful checks and three findings for an unsafe Terraform fixture, then 71 successful checks for the corrected fixture. Empty scans become incomplete. |
| ODiff | Matching, pixel-different, layout-different and invalid inputs produced registered exits 0,22,21,1 respectively. |
| Frictionless | The fixed local CSV parser accepts the runner's normalized path; a malformed extra-cell row produces a finding. |
| Translate Toolkit | The real PO fixture yields translation counts; malformed input fails. `--help` is its readiness probe because `pocount` has no version command. |
| HTTPie | Its trusted update-disabled configuration works as UID 1000 without writing the read-only config directory; TLS verification is explicit. |
| Finance through generic runner | Actual `finance-risk` and `finance-quantlib-bond` commands pass with the intended environments and expected results. |
| First-party analysis | CSV profiling, JSON validation, locale parity, retrieval metrics, redacted privacy heuristics, corpus links, inventory and scaffolding have positive/negative fixtures. |

## Dependency audit and updates

See [SPECIALIST_DEPENDENCIES.md](SPECIALIST_DEPENDENCIES.md) for the dated advisory sources, aliases, exact overrides, compatibility tests and retained findings.

The initial audit covered all 29 default npm tool graphs and 459 distinct Python package/version coordinates across generic and finance locks. High npm chains in Lighthouse CI and Promptfoo were patched with an exact `basic-ftp==6.2.6` override after data/file/HTTP/FTP compatibility tests. Sharp CLI uses patched `sharp==0.35.5`, with an actual image-metadata fixture. Regenerated installed trees report zero high/critical npm findings for those three tools. Moderate and low entries remain documented.

BackstopJS and Checkov are optional because their upstream dependency constraints retain unresolved findings. Semgrep's pinned SDK/telemetry findings and HTTPie's conflicting advisory range are documented with the fixed recipe's applicability. OpenBB's DiskCache finding has no patched upstream release; private owned cache storage is a mitigation, not a patch. No “all dependencies are vulnerability-free” claim is made.

## Independent review closure

Reviews were conducted across catalog recipes, native integration, runner boundaries, installer behavior and finance semantics. Reproduced defects were fixed with focused regressions, including duplicate parameter keys, report-root and parent symlinks, zero-test false passes, Trivy error classification, canonical PostgreSQL invocation, same-named binary caches, stale npm refresh, failed-refresh file preservation, strict lifecycle approvals, numerical conventions and IRR scale stability.

A fresh final reviewer separately found transient discovery recovery, Unicode title truncation, mypy cache mutation and a quick-start effect declaration mismatch. Those fixes are included in the delivered tree and have focused verification. The final image-composition review also identified login-shell profile ordering: the specialist path is now applied after the inherited security profile, and the image smoke explicitly checks the actual `bash -lc` lookup paths. Profile selection does not create a new authorization boundary or change worker images.

## Not executed here: release/deployment gates

| Gate | Status and required environment |
| --- | --- |
| Build all ten Docker targets and combined security/specialist image | **Not run:** this workspace has no Docker/BuildKit daemon. The committed build scripts and manual workflow perform real image builds and smoke checks on an operator host. |
| Resulting image IDs/digests | **Not produced here.** The build receipt records only identities actually observed by Docker. |
| Non-root/read-only image smoke | Script implemented; **not run as a container here**. Host CLI tests do not substitute for it. |
| gVisor, quotas, host firewall/egress qualification | **Not run here.** Use the existing Boxoro worker qualification on the target host/image. |
| apt packages | Image recipes/inventory capture implemented; **not installed or individually version-qualified here**. Noble repository resolution is not a frozen apt snapshot. |
| Live/paid OpenBB providers and third-party MCP accounts | **Not connected/tested.** Keys, account scopes, entitlements, data freshness and redistribution terms remain source-specific. |
| Emulators, real mobile devices, iOS/macOS, GPU model weights, remote databases and cloud accounts | **Not provisioned.** Their catalog entries state the host/service prerequisites. |
| Every recipe against arbitrary project environments | **Not established.** Bundled CLI dependencies do not include each application's imports/plugins/configuration. |
| Production deployment or merge | **Not performed by this change.** Review and qualify the actual image before admission to a worker. |

Run `scripts/build-specialist-image.sh` for the intended family, preserve its receipt, run the existing worker qualification, configure only the intended MCP accounts, then use `specialist doctor` and representative project workflows in a new session. The [operating guide](SPECIALIST_AGENTS.md) describes the complete activation sequence.
