# Security Stack Validation Record

Implementation checked on **2026-11-11 UTC**, based on repository commit `8715651e87137ce82babd91b2307a6eedfeff154` (`work`). This record distinguishes host execution, synthetic boundary tests and deployment qualification. It is not an assertion that the resulting images have been built or deployed.

## Verified implementation

| Area | Evidence | Result |
| --- | --- | --- |
| Rust integration | Full `cargo test --locked` run with the single environment exclusion below | **247 passed**, 9 existing live/qualification tests ignored, 1 environment-specific test filtered |
| Rust formatting | `cargo fmt --all --check` | Passed |
| Rust lint | `cargo clippy --locked --all-targets -- -D warnings` | Passed |
| Runner boundaries and reports | Standard-library tests, real subprocesses, malformed scanner fixtures, private temporary directories | 28 runner tests passed |
| Intelligence MCP | Auth, protocol, strict inputs, provider normalization, size/time limits, credential redaction and loopback Host handling | 28 tests passed |
| Installer integrity | Verified extraction, digest rejection, traversal/link/duplicate rejection and offline/platform failures | 5 tests passed |
| Local web checks | Actual ephemeral loopback HTTP servers; one request, no redirects, safe cookie/header handling and incomplete 4xx responses | 5 tests passed |
| Python lint | Ruff `E4,E7,E9,F,I` over the helper modules, installer, lock generator, MCP and tests | Passed |
| Shell scripts | ShellCheck over image build and smoke scripts | Passed |
| Python dependency lock | Regenerated from verified upstream metadata and compared byte-for-byte | Matched |
| Semgrep installation | `pip install --require-hashes --only-binary=:all:` followed by `pip check` | Semgrep 1.185.0 and 66 transitive runtime dependencies installed; no broken requirements |
| Core artifact installation | Actual release artifacts through the committed checksum/size-verifying installer | Gitleaks, Trivy, Syft, Ruff, ShellCheck and Hadolint installed successfully |

The combined Python verification command is `python3 -m unittest discover -s qa -p 'test_security_*.py' -q`: **66 tests** across runner, web, MCP and installer behavior. The source-layout helper entrypoint was also invoked with isolated Python, `--help` and `{"action":"doctor"}`; both returned successfully, with missing tools at the production `/opt/security/bin` path explicitly visible on this host. This was not a container invocation.

The normal Rust suite's pre-existing `exec_wrapper_keeps_the_command_out_of_process_arguments` test fails with exit 125 in this executor because **`/tmp` does not exist**. Its production wrapper intentionally creates `/tmp/.arexec.*`. Neither that wrapper nor its test was changed to accommodate this host. The successful verification command was:

```sh
CARGO_BUILD_JOBS=2 cargo test --locked -- \
  --skip exec_wrapper_keeps_the_command_out_of_process_arguments
```

A writable task-specific `TMPDIR`, `CARGO_HOME` and `RUSTUP_HOME` were supplied for the local toolchain. The committed manual CI continues to run the full unfiltered Rust suite on a normal Ubuntu runner, followed by every `qa/test_security_*.py` suite. No automatic paid CI trigger was enabled.

## Actual core scanner exercise

The installed binaries were invoked through the real runner against an intentionally insecure, synthetic fixture. The fixture included a fake credential pattern, unsafe Python TLS configuration, an insecure Dockerfile, shell lint defects and manifest supply-chain risks. No real credential was used.

| Check | Observed result |
| --- | --- |
| Gitleaks | Complete, 1 high finding; synthetic secret absent from normalized report artifacts |
| Trivy misconfiguration | Complete, 4 findings |
| Semgrep original rule pack | Complete, 1 finding |
| Syft | Complete; valid sanitized SBOM with 0 components for the fixture's unresolved manifest |
| Static supply-chain heuristics | Complete, 3 findings |
| Ruff | Complete, 1 finding |
| ShellCheck | Complete, 2 findings |
| Hadolint | Complete, 4 findings |
| Rustfmt | Correctly not applicable to this fixture; pinned executable and Rust formatting checked separately |
| Trivy vulnerability scan | Correctly incomplete because no local advisory database was available |

The standard profile returned **16 findings**: 4 high, 7 medium, 2 low and 3 informational. Eight checks completed, one was inapplicable, and one lacked its database. The overall gate was **`incomplete`**, preserving the findings without presenting unavailable vulnerability coverage as a pass.

The focused IaC profile completed both checks and returned **8 findings**, with gate **`fail`** because high-severity findings were present.

### Live database-update result

The explicit update action was exercised against the configured public Trivy repositories. Both main and Java downloads failed because the sanitized scanner process could not reach the environment's DNS server (`network is unreachable`). The action returned **`incomplete`**, and post-download verification reported both databases missing. This is observed network behavior, not evidence of a defective or fresh advisory database.

Live CVE-database scanning, Java archive resolution and production ClamAV signature downloads therefore remain unverified here. Run the documented update action from a worker with public DNS/HTTPS connectivity, confirm database metadata in `doctor`, and repeat the relevant scan.

## Malware runtime exercise

The pinned ClamAV 1.5.4 Debian artifact was downloaded and independently verified against its locked SHA-256 and byte length. Its runtime files were extracted only into a scratch directory; no system-wide package installation occurred. A private library path and the vendor CVD certificate directory were supplied for the host check.

| Check | Observed result |
| --- | --- |
| `clamscan --version` | Exit 0, ClamAV 1.5.4 |
| `freshclam --config-file=/dev/null --version` | Exit 0, ClamAV 1.5.4 |
| Synthetic EICAR input with one locally authored `.ndb` test signature | Exit 1 and the expected unofficial test-signature match |
| Harmless input with that same test signature | Exit 0 |
| Empty signature directory | Exit 2 |
| Actual runner parser on the positive match | One normalized high-severity finding |

This exposed and fixed a packaging issue: the vendor package does not install a live FreshClam configuration file, so plain `freshclam --version` fails. The Dockerfile and image smoke script now use the verified explicit empty configuration for version-only probes. The runtime database updater retains its separate fixed configuration.

These checks used **one synthetic test signature**, not production antivirus signatures. They establish runtime loading, exit-code interpretation and parser behavior, not broad malware detection or an executable container image.

The bounded YARA build check stopped because this executor lacks autoconf, autoreconf, automake and libtoolize. No host packages were installed to work around that restriction. The image's disposable builder declares those dependencies, but its compile/link result remains unverified.

## Independent review and fixes

Independent reviews covered native dispatch and schemas, worker boundaries, scanner parsing, cancellation, image installation, MCP routing, credential handling and operator documentation. Confirmed defects were reproduced before correction:

- Credential-shaped filenames in snapshot metadata bypassed the redactor. Skipped/excluded paths now pass through it, with redaction before truncation.
- A terminated runner could leave its scanner process group alive. TERM/INT/HUP now trigger bounded child cleanup and unwind temporary state.
- Malformed linter objects could be interpreted as findings and still pass. Required fields and severity/location schemas are now validated.
- A successful updater exit could be accepted without database files. Both Trivy databases are now checked for real artifacts, schemas and freshness.
- The helper's JSON envelope was smaller than a valid escaped native path. The envelope now allows 32 KiB while retaining the 4096-byte path limit.
- ShellCheck and Trivy inline directives could silently hide findings. Actual installed-binary reproductions now produce `suppressed` status and an incomplete gate. Rustfmt skip directives receive the same conservative treatment.
- MCP port 80 configuration rejected a standard client's bare default-port Host header. Bare loopback hosts are now accepted only on port 80; unrelated hosts and mismatched ports remain rejected.
- Readiness guidance exceeded its evidence. `doctor` now separates expected and installed manifests, file/executable availability and observed process/cgroup resources from deployment qualification.
- Malformed database timestamp types could raise unstructured exceptions. Timestamp type/timezone validation and regression coverage now produce structured errors.

Additional checks cover planted cache links, missing tools, timeout/output limits, snapshot bounds, raw-secret omission, stable fingerprints, JSON/SARIF output, and normalized report sizes below Boxoro's download ceiling.

## Deployment checks still required

This executor has **no Docker CLI/daemon**, and no image was built, published, selected on a worker or deployed. The following are explicit release requirements:

1. Build the `core` and `malware` Dockerfile targets using `scripts/build-security-image.sh` on a Docker/BuildKit host.
2. Run the included smoke tests against the actual image ID, then repeat with `BOXORO_SMOKE_RUNTIME=runsc` on a configured worker.
3. Complete Boxoro's existing host, network, lifecycle and resource qualification for that image.
4. Populate current advisory/signature databases, run a real dependency-vulnerability fixture and a production-signature malware fixture, and measure peak memory/disk needs.
5. Review the derived image's own vulnerabilities, licenses, provenance and registry signature before admitting its actual digest.
6. Configure any optional intelligence account on the control side and verify its permitted live endpoints and plan. The MCP test suite uses bounded provider fixtures, not paid production accounts.

Hadolint parses the security Dockerfile. Its sole remaining policy finding is **DL3008** for unpinned Ubuntu build packages in the disposable YARA builder; the recipe is integrity-pinned for scanner artifacts but is not advertised as a bit-for-bit reproducible apt build.

The [operating guide](SECURITY_STACK.md), [research](SECURITY_STACK_RESEARCH.md) and [intelligence guide](SECURITY_INTELLIGENCE.md) contain the actual commands, manifests, limitations and deployment settings. An image's local Docker config ID must not be substituted for a registry manifest digest.
