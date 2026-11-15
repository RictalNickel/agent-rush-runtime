# Specialist verification, practical uses and efficiency

Reviewed **2026-11-15**. This follow-up checks whether the tools fit their assigned work, corrects reproduced command/reporting defects, adds useful operations from the existing installations, and measures avoidable runtime and installation costs. The comparison starts at local commit `c40250453c66fe320d64e40acda782cbff2a5c85`, whose tree matches published commit `cba870c2751508f2253c6eeb189122b88b440a2f`.

## Result and scope

The catalog now defines **21 specialist profiles, 167 distinct tool entries, 156 fixed operations, 77 named workflows and 10 connector references**. Five operations and nine profile workflow entries were added without changing dependency versions or locks. Every profile's operating guide now recommends task-scoped discovery and readiness, and explains practical applications and prerequisites. The native tool, selected-specialist guidance and new-session UI use the same discovery model.

The catalog distinguishes executable adapters from installation recipes and references. **120 tool IDs have a registered operation; 47 do not.** The latter include optional libraries, all host/external prerequisites and the adb client. An installed library or a connector record does not establish a native adapter, connected account or working device. The 118 image entries, 34 optional entries, 11 host prerequisites and four external entries retain their existing availability classifications.

This is a review of the implementation and qualified fixtures, not a certification of every possible input or deployment. The [tool-fit report](SPECIALIST_TOOL_REVIEW.md) contains the profile-by-profile assessment, exact reproductions, CLI versions and primary sources. The [initial implementation validation](SPECIALIST_VALIDATION.md) remains a separate historical record.

## Correctness fixes that affect real work

| Area | Reproduced problem | Implemented behavior |
| --- | --- | --- |
| Markdown scope | A file target could match zero files. Bracketed file/directory names could select and rewrite an unselected sibling. | A dedicated literal-file operation, a fixed directory-relative recursive glob, directory-target preflight, and positive file-count qualification. Real fixtures confirm unchanged siblings. |
| Markdown effects | Project `fix:true` rewrote a file even without a command-line fix flag. Configured globs could broaden selection. | Both recipes declare project code and mutation; `--no-globs` suppresses additional configured globs. Custom rules remain project code, subject to the worker boundary. |
| TypeScript cycles | Madge's default directory discovery missed a two-file TypeScript cycle and returned an empty successful result. | Explicit JS/JSX/TS/TSX/MJS/CJS extensions. The same cycle fails and its corrected version passes. |
| Translation coverage | i18next returned 100% coverage after skipping malformed source or after an extraction plugin failed. Missing translations used an unrecognized findings exit. | Missing/unused keys are findings; malformed configuration, partial parsing and plugin failure diagnostics are incomplete. Positive key counts cannot override those failures. |
| Protobuf | Buf lint findings used exit 100, and configuration could launch a local plugin. | Correct findings codes and declared project-code effects. Compatibility checks reject compile failures as incomplete. |
| Profiling | Memray/Pyinstrument exit 1 from a failed workload was treated as a completed performance finding. | Failed workloads remain incomplete; existing diagnostic captures are retained. No unconfigured performance budget is inferred. |
| Translation metrics | Identical Chinese text scored BLEU 0 under the fixed default tokenizer. | A bounded built-in tokenizer choice; the Chinese fixture scores 100 with `zh`, and the metric signature is preserved. |
| Pipeline inventory | dlt inspected the fresh runner home instead of the supplied pipelines directory. | The command explicitly selects the requested directory. Inventory is still distinct from a successful pipeline run. |
| Reinstallation | pip accepted a modified installed package as already satisfied and kept an unlocked old dependency. | Recreate the isolated tool venv before the hashed install, then run `pip check`. Real offline fixtures verify repair and rejection of tampered wheel bytes. |
| Interrupted downloads | A partial release download poisoned the reusable cache entry. | Verify a temporary download before atomic publication; clean failed partial files and recheck reused artifacts. |

The independent reviewer reproduced three additional issues during this work: Markdown glob scope, partial i18next extraction, and failed profiling workloads. All were corrected and rechecked against actual installed tools. The closure review also checked that finance schema validation stays in its bounded query process.

## Additional useful workflows

| Profile | New operation or exposed workflow | Suitable use |
| --- | --- | --- |
| API Architect | `buf-breaking` / `protobuf-compatibility` | Compare a local candidate schema with a supplied exact local baseline before changing a consumer contract. |
| API Architect | `redocly-bundle` / `bundle-contract` | Produce one JSON review artifact from a specification with multiple files. Missing references fail; remote references and preprocessors retain declared effects. |
| Database Pro and relevant data profiles | `duckdb-columns` / expanded `duckdb-schema` | Inspect types, order, nullability and defaults without changing the database file. |
| LocaleQA | `i18next-unused` | Identify candidate obsolete messages for review without deleting them. Dynamic keys and incomplete extraction still require attention. |
| DocSmith | `markdownlint-file` / `markdown-file` | Check one changed document instead of its entire project. |
| API Architect | `graphql-compatibility` | Expose the existing local schema comparison as a named workflow. |
| Performance Lab | `memory-profile`, followed by `memory-report` | Capture a real Python allocation workload, then inspect its retained evidence. |
| DataWorks | Two-step `csv-quality`; optional `csv-statistics` | Run bounded profiling and schema validation routinely; request richer csvkit statistics when they answer the task. |

The first five rows add operations; the remaining rows expose or improve existing capabilities. These are fixed registered commands using the existing locks. Generated `.specialists` artifacts must be verified and copied through an authorized file operation to an ordinary project path before reuse as input. A workflow does not automatically wire output artifacts into later requests.

The [Finance guide](FINANCE_AGENT.md#choosing-a-practical-workflow) now maps existing tools to portfolio-risk analysis, factor exposure, bond-yield sensitivity, valuation scenarios, allocation holdouts and forecast evaluation. Retrieve and document one appropriate source sample, then reuse the fixed local numerical operations for comparable scenarios. `max_rows` bounds returned OpenBB data; it does not narrow an upstream fetch. Dates, symbols and other provider parameters should constrain retrieval at its source.

## Focused discovery and readiness

`describe` and `doctor` now accept either an `operation` or a `workflow`. Selectors must belong to the selected profile and cannot be combined. Existing unscoped calls still return/check the full profile.

```json
{"tool":"specialist","action":"describe","agent":"testforge","workflow":"python-gate"}
```

```json
{"tool":"specialist","action":"doctor","agent":"testforge","workflow":"python-gate","timeout_seconds":60}
```

Read the returned contracts and `required_effects`, supply the actual project path and parameters, and declare the effects required for the authorized task. The scoped result includes selected operations/tools and explicitly lists excluded profile tools. Several workflow steps using one tool result in one readiness probe. HTTP/MCP catalog-discovery endpoints retain full-profile behavior; selectors apply to the native tool and helper.

The [measurement receipt](validation/specialist-scope-efficiency.json) compares TestForge's full profile with its `pytest-test` operation using three alternating repetitions and the installed Python/native prefix:

| Measurement | Full profile | Selected pytest operation |
| --- | ---: | ---: |
| Serialized description | 29,110 bytes | 2,402 bytes |
| Operation contracts | 23 | 1 |
| Tool records / readiness checks | 15 | 1 |
| Median doctor elapsed time | 1.486499 s | 0.090773 s |
| Readiness outcome on this prefix | 9 passed, 6 missing; incomplete | 1 passed; pass |

The selected description is **91.75% smaller by serialized bytes**. This is not a token-count measurement. The narrower readiness check avoids unrelated probes; it does not make a full-profile check faster or hide missing tools. The receipt retains every observed status, the exact catalog hash and all elapsed samples. Host timings depend on caches and concurrent work and are not a deployment service-level objective.

Reproduce from the repository root, substituting the qualified installation prefix:

```sh
python3 qa/benchmark_specialist_scope.py \
  --agent testforge --operation pytest-test \
  --tool-bin /opt/box/agents/bin --repetitions 3 \
  --output specialist-scope-efficiency.json
```

## OpenBB startup and repeated discovery

Profiling found that an offline query's actual calculation took approximately 0.1 seconds while importing all providers and generating the complete schema/catalog took tens of seconds. The service now builds approved metadata once in a credential-free, network-disabled bootstrap child. It keeps schemas and descriptions in memory for its lifetime.

Warm status, search and describe calls reuse those schemas. **Every data query still starts a fresh bounded child.** Exact schema validation and an installed-package fingerprint check run before the SDK import. The parent never loads the SDK, accepts a caller-supplied contract or retains provider responses. Concurrent cold discovery shares one bootstrap; failure is retryable; stale installations invalidate the cache; lock waits and cold-query bootstrap/query work consume the same total request deadline.

The final actual authenticated HTTP check used all 17 installed provider packages, 1,138 approved commands in 23 namespaces, no provider credentials, and offline `quantitative.summary`. Its result remained count 4, mean 2.5 and sample variance 5/3 with real OBBject metadata and matching schema/package fingerprints.

| Authenticated HTTP measurement | Baseline | Final revision |
| --- | ---: | ---: |
| Cold status | 39.863 s | 37.528 s |
| Warm status | Another full SDK/schema worker | 1.61 ms |
| Warm catalog | Another full SDK/schema worker | 2.05 ms |
| Warm describe | Another full SDK/schema worker | 1.65 ms |
| Fresh-worker offline query after discovery | 38.483 s | 27.802 s |

The final query sample is **27.76% lower in elapsed time**. These are individual same-host development measurements, not a distribution or a live provider benchmark. The [finance receipt](validation/finance-efficiency-2026-11-15.json) separates authenticated HTTP from executor-only timing and identifies the measured source files by SHA-256.

Caching has a real cost: the parent retains metadata. An earlier revision measured parent peak RSS rising from approximately 19 MiB to 63 MiB and query-child peak falling from approximately 1,085 MiB to 867 MiB. Those memory samples preceded the final schema-validation refinement and are labeled accordingly; only HTTP correctness/timing was rerun on the final revision. Bootstrap still needs the full one-time SDK/schema memory peak. Restart after immutable package updates, run `openbb_status` once as a deployment-readiness check, and budget for combined startup when the first request is a query.

## Installation and build efficiency

The [installation review](SPECIALIST_INSTALLATION_REVIEW.md) records the implemented cache, layer and integrity changes:

- Python wheel installs can reuse an explicit pip download cache while retaining hash checks and clean isolated environments. Source-build opt-ins continue to bypass wheel-cache reuse.
- Image builds use a platform/version-specific, locked BuildKit download-cache mount. Ordinary helper calls still remove their temporary default cache; persistent caches are an explicit choice.
- The Node-install layer depends on its installer and pinned Node inputs rather than every changing runtime/catalog file.
- The specialist Dockerfile excludes host Python bytecode from its context.
- Release hashing streams 1 MiB chunks. The same 32 MiB fixture reduced peak traced Python allocation from **33,559,866 to 2,103,198 bytes**, about 16-fold, with the same hash.
- A fresh offline installation of locked Prettier, Buf and Hadolint from existing caches completed in **2.389 seconds**, and all three actual version probes passed. This establishes functional cache reuse; it is not a measured cold/warm build comparison.

The [installation receipt](validation/specialist-installation-review.json) retains measurements and exact limits. BuildKit cache mounts are builder-local and are not automatically exported through normal layer-cache options. No Docker build-time or final-image-size improvement is claimed without a real image build.

## Final verification

The integrated tests cover the changed specialist code and the existing security stack together. Counts below overlap where a focused backend or real-tool suite is part of the combined Python suite; they must not be added together.

| Verification | Observed result |
| --- | --- |
| Rust unit/integration suites | 258 passed; nine existing host/live tests ignored; one known missing-`/tmp` environment test filtered. Production wrapper behavior is unchanged. |
| Combined Python, with real specialist CLI fixtures enabled | 217 tests: 213 passed, four optional-backend/SDK skips. |
| Installed CLI recipe regressions, included above | 19 passed, including Markdown scope/mutation, translation extraction, TypeScript cycles, Buf, profiler failure, bundling and immutable database inspection. |
| Installation tests, included above | 14 passed, including real wheel replacement/tamper and interrupted-download recovery. |
| Finance in the locked OpenBB environment | 64 tests: 62 passed, QuantLib and opt-in SDK skipped. |
| Finance in the advanced environment | Six passed, including real statsmodels, SciPy and QuantLib calculations. |
| Final authenticated OpenBB SDK smoke | Passed separately with the opt-in enabled; actual offline calculation, metadata, cold/warm discovery and fingerprints checked. |
| Browser suite | All 12 passed against the real Rust-served UI and fixture session API, including desktop/mobile specialist selection and discovery recovery. No worker was connected. |
| Native formatting, Clippy and build | Passed: `cargo fmt --all --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo build --locked`. |
| Scoped Ruff, shell/JavaScript syntax | Passed. |
| Redacted Gitleaks scan of the follow-up patch | Zero matches; inline allow comments were ignored for this scan. |
| Dockerfile lint | No errors. Existing base-stage DL3002/DL3066 warning/information remain. |
| Independent closure review | 11 focused tests passed plus an additional real unused-translation plugin-failure reproduction; no remaining concrete issue in the reviewed fixes. |

The [integrated verification receipt](validation/specialist-review-tests.json) records commands, outcomes, environment limits and hashes of changed executable sources. The separate measurement receipts retain their original observations rather than combining results from different methods or revisions.

Run the broad Python and native suites from the repository root:

```sh
BOXORO_TEST_SPECIALIST_TOOLS=1 python3 -m unittest discover -s qa -p 'test_*.py' -v
cargo test --locked --tests -- --skip exec_wrapper_keeps_the_command_out_of_process_arguments
```

The named Rust skip is specific to this workspace, which has no `/tmp`; test subprocess temporary files use a private `TMPDIR` below the workspace. The original failure and its unchanged production wrapper are documented in [initial validation](SPECIALIST_VALIDATION.md). On a normal deployment host, run the unfiltered suite. Real-CLI test prefixes and opt-ins are documented in [the tool review](SPECIALIST_TOOL_REVIEW.md#executed-validation-and-reproduction); finance commands are recorded in its measurement receipt.

## Remaining qualification

No Docker/BuildKit daemon is available in this environment. The ten image targets, combined security/specialist image, container smoke tests, worker/gVisor/network controls and actual image resource use still require the deployment host. Docker source lint and host CLI tests do not establish those outcomes.

No paid/live provider request, external MCP account, device/emulator, cloud account or remote database was provisioned by this follow-up. Optional components retain their documented prerequisites. Dependency locks did not change; the existing advisory findings and retained upstream constraints, including the OpenBB DiskCache mitigation, remain documented in [SPECIALIST_DEPENDENCIES.md](SPECIALIST_DEPENDENCIES.md) and [OPENBB_PROVIDERS.md](OPENBB_PROVIDERS.md). This review does not claim a fresh vulnerability-free audit.

See [SPECIALIST_AGENTS.md](SPECIALIST_AGENTS.md) for image selection and [SPECIALIST_RUNTIME.md](SPECIALIST_RUNTIME.md) for the exact request, effect, result and resource contracts.
