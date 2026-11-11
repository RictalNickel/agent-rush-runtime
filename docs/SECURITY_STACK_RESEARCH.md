# Boxoro defensive security stack: research and packaging decisions

Research snapshot: **11 November 2026**. This document distinguishes installed capabilities, optional integrations, and checks that still require a running image. Operational instructions are in [SECURITY_STACK.md](SECURITY_STACK.md). The machine-readable source of version, URL and artifact digest pins is [tools.lock.json](../deploy/security/tools.lock.json).

## 1. Recommended distribution

The useful default is a static analysis workspace: Gitleaks for exposed-secret patterns; Semgrep CE with original local rules for selected dangerous code patterns; Trivy for known dependency vulnerabilities and infrastructure configuration; Syft for package inventory; and Ruff, ShellCheck, Hadolint and Rustfmt for language/container hygiene. ClamAV and YARA belong in the optional `malware` image target. Dependency update bots and authenticated intelligence belong outside the credential-free box.

These checks produce evidence about particular risks. They cannot establish that a project contains no malware, that all secrets have been found, that dependencies are authentic, or that an application is secure. Unknown malicious code, environment-triggered behavior, unavailable advisories, incomplete lockfiles, encrypted archives, unsupported formats and intentionally obfuscated code can evade the checks. An SBOM describes observed components; it is not an attestation of their safety. Treat missing tools, stale databases, parser errors, skipped content and timeouts as incomplete coverage.

### Included and optional tools

| Tool and researched pin | Distribution | Role and useful limit | Upstream source/license |
| --- | --- | --- | --- |
| Gitleaks **8.30.1** | Core | Pattern/entropy-based secret detection. Finding an apparent key does not establish validity, theft or use. Working-tree scans do not cover deleted Git history. | [Release](https://github.com/gitleaks/gitleaks/releases/tag/v8.30.1), [MIT license](https://github.com/gitleaks/gitleaks/blob/v8.30.1/LICENSE) |
| Semgrep CE **1.185.0** | Core | Static rules for supported languages. The bundled original rules cover selected risky constructs, not a complete enterprise SAST ruleset or all interprocedural behavior. | [Release](https://github.com/semgrep/semgrep/releases/tag/v1.185.0), [licensing](https://docs.semgrep.dev/licensing) |
| Trivy **0.76.2** | Core | Known package vulnerabilities, IaC/container misconfiguration and image analysis. Requires appropriate fresh databases and recognizable package metadata. | [Release](https://github.com/aquasecurity/trivy/releases/tag/v0.76.2), [Apache-2.0](https://github.com/aquasecurity/trivy/blob/v0.76.2/LICENSE) |
| Syft **1.57.1** | Core | CycloneDX/SPDX inventory from supported files and images. Completeness depends on available manifests, installed metadata and cataloger coverage. | [Release](https://github.com/anchore/syft/releases/tag/v1.57.1), [Apache-2.0](https://github.com/anchore/syft/blob/v1.57.1/LICENSE) |
| Ruff **0.17.4** | Core | Python lint; complements security rules without executing the project. | [Release](https://github.com/astral-sh/ruff/releases/tag/0.17.4), [MIT and bundled notices](https://github.com/astral-sh/ruff/blob/0.17.4/LICENSE) |
| ShellCheck **0.11.0** | Core | Shell analysis, including unsafe quoting and common shell mistakes. | [Release](https://github.com/koalaman/shellcheck/releases/tag/v0.11.0), [GPL-3.0](https://github.com/koalaman/shellcheck/blob/v0.11.0/LICENSE) |
| Hadolint **2.15.1** | Core | Dockerfile best-practice checks, including embedded shell analysis. | [Release](https://github.com/hadolint/hadolint/releases/tag/v2.15.1), [GPL-3.0](https://github.com/hadolint/hadolint/blob/v2.15.1/LICENSE) |
| Rustfmt from Rust **1.90.0** | Core | Formatting check using the inherited pinned toolchain. It does not provide Rust semantic security analysis. | [Rustfmt project](https://github.com/rust-lang/rustfmt), [toolchain manifest](https://static.rust-lang.org/dist/channel-rust-1.90.0.toml) |
| ClamAV **1.5.4** | Malware target | Signature and heuristic file scanning; useful for known malware and suspicious archives. No guarantee against novel or evasive malware. | [Release](https://github.com/Cisco-Talos/clamav/releases/tag/clamav-1.5.4), [GPL-2.0 and bundled licensing](https://github.com/Cisco-Talos/clamav/blob/clamav-1.5.4/COPYING.txt) |
| YARA **4.6.0** | Malware target | Matches explicit byte/text/structure rules. An indicator is a triage lead; no match means only that the supplied rules did not match. | [Release](https://github.com/VirusTotal/yara/releases/tag/v4.6.0), [BSD-3-Clause](https://github.com/VirusTotal/yara/blob/v4.6.0/COPYING) |
| OSV Scanner **2.7.1** | Optional separate installation | Alternative known-vulnerability matching against OSV; useful when OSV-specific coverage or independent comparison is needed. | [Release](https://github.com/google/osv-scanner/releases/tag/v2.7.1), Apache-2.0 |
| Grype **0.121.3** | Optional separate installation | Vulnerability matching from a Syft SBOM; overlaps substantially with Trivy. | [Release](https://github.com/anchore/grype/releases/tag/v0.121.3), Apache-2.0 |
| TruffleHog **3.99.8** | Optional separate installation | Additional credential detectors and optional provider verification. Verification contacts services using candidate secrets, so it is excluded from default scans. | [Release](https://github.com/trufflesecurity/trufflehog/releases/tag/v3.99.8), [AGPL-3.0](https://github.com/trufflesecurity/trufflehog/blob/v3.99.8/LICENSE) |

Gitleaks currently describes itself as feature complete and maintained for security fixes, with its maintainer moving new work to Betterleaks. Keeping it pinned is a deliberate continuity decision; track its maintenance status instead of assuming new detectors will continue arriving. [Source: upstream README](https://github.com/gitleaks/gitleaks).

## 2. Installation, integrity and resource budget

`deploy/security.Dockerfile` derives from an operator-supplied workspace image and preserves uid/gid `1000:1000`, the read-only installed tools, and the existing `/workspace` boundary. The supported image platform is **linux/amd64**. Upstream arm64 artifacts exist for many scanners, but the inherited workspace downloads an x86_64 Rust toolchain; arm64 is not claimed as supported without changing and validating that base.

The six core native release downloads total **160,774,236 bytes** before extraction, according to the sizes recorded in the lock. The Semgrep amd64 wheel is about 72 MB, before its dependency wheels and installed files. These are incremental downloads, not an image-size prediction: the inherited image already contains browsers, office tools and compilers. [Semgrep wheel and published hash](https://pypi.org/project/semgrep/1.185.0/).

The installer checks exact SHA-256 and expected size before using each release asset. It selects a single regular executable from each archive, rejects links and traversal paths, preserves license notices, and records the installed identities in `/opt/security/versions.json`. It does not pipe remote installer scripts into a shell. The pinned upstream license files also have verified hashes. GitHub release asset digests are authenticated metadata, not an independent audit of the publisher or a guarantee that a release itself is benign.

This distinction matters: Aqua disclosed a March 2026 compromise affecting Trivy 0.69.4 release binaries and other mutable distribution surfaces. The chosen 0.76.2 release is a later immutable GitHub release; the installer still requires its exact digest. Maintain a reviewed lock-update procedure and inspect upstream security notices. Where upstream publishes Sigstore bundles, independent signature/provenance verification can be added to the release-admission process. [Aqua incident disclosure](https://www.aquasec.com/blog/trivy-supply-chain-attack-what-you-need-to-know/), [pinned release](https://github.com/aquasecurity/trivy/releases/tag/v0.76.2).

Semgrep's complete runtime dependency set is exported from the **verified upstream 1.185.0 `cli/uv.lock`**, using uv 0.13.6 with `--offline --frozen --no-dev --no-emit-project`. The selected Linux wheel hash is appended explicitly. `requirements.lock` is installed with `--require-hashes --only-binary=:all:`; no Python source distribution is built. `lock-python.py --source-dir ... --check` can reproduce the lock entirely offline. The upstream lock and project file hashes are in `tools.lock.json`. [Upstream project](https://github.com/semgrep/semgrep/blob/v1.185.0/cli/pyproject.toml), [upstream frozen lock](https://github.com/semgrep/semgrep/blob/v1.185.0/cli/uv.lock).

YARA does not publish a Linux binary in the selected release. The malware builder fetches exactly commit `6d47342e825e59f5d8f20874d0778332bcec83a6`, verifies the fetched commit, and compiles it in a disposable stage. Its pin is a **Git SHA-1 object identity**, not an invented archive SHA-256. The final target receives its executables and notices without the extra build tools. Process scanning, Cuckoo and libmagic support are disabled in this file-scanning build; crypto-supported file modules remain available. [YARA release](https://github.com/VirusTotal/yara/releases/tag/v4.6.0), [configure options](https://github.com/VirusTotal/yara/blob/v4.6.0/configure.ac).

The official ClamAV Debian package has its external library dependencies statically included and installs under `/usr/local`. It does not provide a ready-to-use signature directory or FreshClam configuration. The image links its installed commands into `/opt/security/bin`, starts no daemon, and includes no mutable signatures. The final malware layer removes the package's unused `libclamav_rust.a` build archive (376,238,762 bytes in the verified package), retaining the runtime shared libraries and license notices. [Official package instructions](https://docs.clamav.net/manual/Installing.html), [verified package release](https://github.com/Cisco-Talos/clamav/releases/tag/clamav-1.5.4).

ClamAV's own published recommendation is **3–4 GiB RAM and 5 GiB free disk**. A Boxoro worker also needs room for its source snapshot, reports, Trivy databases and other tools. Start malware workers at **at least 8 GiB memory and 12 GiB writable workspace**, then measure real repositories and archives; these combined values are an engineering starting point, not a measured minimum. Run scanners sequentially. Do not silently retain Boxoro's 4 GiB defaults and claim full malware coverage. [ClamAV requirements](https://docs.clamav.net/Introduction.html).

### Image identity and reproducibility

The build script accepts an immutable registry reference or a local Docker image ID. When no base is supplied it builds the existing workspace Dockerfile, tags that local result uniquely, and verifies its identity before and after the security build. It records the actual Docker config `image_id` separately from observed `RepoDigests`. A local build may have no registry digest. Do not format a config digest as `repository@sha256:...` and call it a registry manifest.

Pinned tool inputs do not make the entire build byte-for-byte reproducible: the base's build process and the malware builder's signed Ubuntu package repositories are separate inputs. Use a reviewed base digest, retain the emitted metadata, and rebuild on patched bases. The script never pushes an image. Optional build-proxy certificates are transient BuildKit secrets; TLS verification remains enabled. `BOXORO_BUILD_CA_FILE` requires an explicit `--base`, because the existing workspace Dockerfile does not consume the proxy-CA secret; prepare that base separately under its own trusted build environment.

## 3. CLI behavior that affects a safe runner

The following recipes were checked against the pinned upstream source or official documentation. They are low-level diagnostic examples; the native `security_scan` tool supplies fixed paths, bounded snapshots, timeouts and redacted reports.

### Secrets

```sh
gitleaks dir /workspace/project \
  --config /opt/box/security/rules/gitleaks.toml \
  --gitleaks-ignore-path /dev/null --ignore-gitleaks-allow \
  --redact=100 --report-format json --report-path /workspace/gitleaks.json
```

In 8.30.1, specifying `--gitleaks-ignore-path` does **not** prevent a second load of `SOURCE/.gitleaksignore`. The runner therefore excludes repository scanner-control files from its private snapshot and reports the exclusions. `--ignore-gitleaks-allow` handles inline suppressions. Full-history auditing requires an independently bounded complete local Git checkout; `gitleaks git` is a distinct operation. [Pinned command implementation](https://github.com/gitleaks/gitleaks/blob/v8.30.1/cmd/root.go).

TruffleHog can add detection coverage, but keep `--no-verification` for an offline analysis workflow and scrub raw findings before they reach the model. Provider validation is a separate, explicitly scoped operation: it creates network traffic and may leave provider audit events. [Upstream usage and verification discussion](https://github.com/trufflesecurity/trufflehog).

### Dependencies, SBOM and infrastructure

```sh
trivy fs --config '' --ignorefile '' --disable-telemetry \
  --cache-dir /workspace/.security/cache/trivy \
  --scanners vuln,misconfig --format json --output /workspace/trivy.json \
  --skip-db-update --skip-java-db-update --skip-check-update --offline-scan \
  /workspace/project

SYFT_CHECK_FOR_APP_UPDATE=false syft \
  --config /opt/box/security/rules/syft.yaml \
  dir:/workspace/project -o cyclonedx-json=/workspace/sbom.cdx.json
```

Trivy's empty `--config ''` and `--ignorefile ''` explicitly disable loading those files in the pinned implementation. `--offline-scan` alone is not a complete network barrier: database/check updates need their own skip flags. An absent database must fail or mark coverage incomplete. Database updates are a separate online action; retain their freshness metadata. [Global flags](https://github.com/aquasecurity/trivy/blob/v0.76.2/pkg/flag/global_flags.go), [report flags](https://github.com/aquasecurity/trivy/blob/v0.76.2/pkg/flag/report_flags.go), [air-gap documentation](https://trivy.dev/docs/latest/advanced/air-gap/), [database options](https://trivy.dev/docs/latest/configuration/db/).

Disable telemetry explicitly, not just application-update checks. Trivy now documents telemetry controls. [Telemetry documentation](https://trivy.dev/docs/latest/advanced/telemetry/).

Java artifact identification uses a separate SQLite file, `CACHE/java-db/trivy-java.db`, and `CACHE/java-db/metadata.json` with schema `Version: 1`, `UpdatedAt`, `NextUpdate` and `DownloadedAt`. The dependency pinned by Trivy 0.76.2 sets `NextUpdate` 72 hours after `UpdatedAt`; the current publisher builds daily. Trivy's client also accepts a database downloaded within the last 24 hours. For a coverage gate, verify the actual nonempty database and sane schema/timestamps, cap content age at 72 hours, and do not treat a recent download timestamp alone as fresh content. The main vulnerability database's policy is separate. Update Java data explicitly with `trivy image --download-java-db-only --cache-dir CACHE`. [Pinned metadata](https://github.com/aquasecurity/trivy-java-db/blob/184bd7481d48/pkg/db/metadata.go), [database schema](https://github.com/aquasecurity/trivy-java-db/blob/184bd7481d48/pkg/db/db.go), [72-hour producer interval](https://github.com/aquasecurity/trivy-java-db/blob/184bd7481d48/pkg/builder/builder.go), [Trivy client policy](https://github.com/aquasecurity/trivy/blob/v0.76.2/pkg/javadb/client.go), [publication schedule](https://github.com/aquasecurity/trivy-java-db/blob/main/.github/workflows/cron.yml).

The trusted Syft configuration disables update checks, Java network enrichment and local Maven repository access, and JavaScript/Python remote-license queries. Static directory cataloging must remain inside the sandbox; no universal guarantee about every future cataloger is assumed. SBOMs can expose private component names, versions and paths, so keep them owner-scoped and sanitize unnecessary properties. [Configuration reference](https://oss.anchore.com/docs/reference/syft/configuration/), pinned [Java](https://github.com/anchore/syft/blob/v1.57.1/syft/pkg/cataloger/java/config.go), [JavaScript](https://github.com/anchore/syft/blob/v1.57.1/syft/pkg/cataloger/javascript/config.go) and [Python](https://github.com/anchore/syft/blob/v1.57.1/syft/pkg/cataloger/python/config.go) options.

OSV Scanner V2 uses `osv-scanner scan source -r PATH --format json --output-file REPORT`. Offline mode is `--offline-vulnerabilities`; `--download-offline-databases` is a separate network-requiring step. Do not copy older V1 command recipes or enable `fix` on untrusted code: guided remediation can invoke package managers and project scripts. [V2 usage](https://google.github.io/osv-scanner/usage/), [offline mode](https://google.github.io/osv-scanner/usage/offline-mode/).

Grype can consume `grype sbom:sbom.cdx.json -o json`, avoiding another source inventory. Its database policy includes a maximum-age check; keep that check enabled and report errors as incomplete. Adding Grype and OSV to every run duplicates much of Trivy's work and increases database storage and update traffic. Use them when their independent coverage resolves a concrete need. [Grype database guidance](https://oss.anchore.com/docs/guides/vulnerability/database/), [upstream usage](https://github.com/anchore/grype).

### SAST and lint

```sh
semgrep scan --oss-only --config /opt/box/security/rules/semgrep.yml \
  --metrics off --disable-version-check --no-secrets-validation \
  --x-ignore-semgrepignore-files --no-git-ignore --disable-nosem \
  --jobs 2 --max-memory 512 --timeout 10 --error --json \
  --output /workspace/semgrep.json /workspace/project

ruff check --isolated --no-cache --no-respect-gitignore --ignore-noqa \
  --output-format json /workspace/project
shellcheck --norc --format=json /workspace/project/script.sh
hadolint --config /opt/box/security/rules/hadolint.yaml \
  --disable-ignore-pragma --format json /workspace/project/Dockerfile
```

Semgrep's experimental `--x-ignore-semgrepignore-files` is present in 1.185.0; pin and test it when upgrading. The output option also accepts URLs, which is why the runner only supplies its own local destination. Memory/time flags limit particular scanner work, not every allocation of the whole process; retain container and process limits. Avoid `--config auto`, cloud logins, autofix and local-build options in the default path. [Pinned scan implementation](https://github.com/semgrep/semgrep/blob/v1.185.0/cli/src/semgrep/commands/scan.py), [CLI reference](https://docs.semgrep.dev/cli-reference), [metrics behavior](https://docs.semgrep.dev/metrics).

Ruff's isolated mode bypasses configuration files; its Git-ignore and inline-noqa controls are separate. ShellCheck should not receive `--external-sources`; Hadolint needs a trusted explicit configuration and disabled ignore pragmas. Formatting checks do not replace SAST. `cargo check`, Clippy and test/build commands can run project build scripts or procedural macros, so reserve them for a separately scoped remediation/build step. [Pinned Ruff flags](https://github.com/astral-sh/ruff/blob/0.17.4/crates/ruff/src/args.rs), [ShellCheck manual](https://github.com/koalaman/shellcheck/blob/v0.11.0/shellcheck.1.md), [Hadolint usage](https://github.com/hadolint/hadolint/blob/v2.15.1/README.md), [Cargo build-script behavior](https://doc.rust-lang.org/cargo/reference/build-scripts.html).

### Malware triage

```sh
clamscan --recursive --infected --no-summary \
  --database=/workspace/.security/cache/clamav \
  --alert-exceeds-max=yes /workspace/project
yara -r -N -a 60 /opt/box/security/rules/indicators.yar /workspace/project
```

Use explicit archive/file-size limits and report exceeded limits. Do not remove or quarantine files automatically. ClamAV returns 0 for no match, 1 for matches and 2 for errors; YARA can return 0 when rules match, so parse bounded results and error diagnostics rather than treating exit zero as clean. A database update uses FreshClam explicitly, never as a side effect of a normal scan. [ClamAV manual](https://github.com/Cisco-Talos/clamav/blob/clamav-1.5.4/docs/man/clamscan.1.in), [YARA CLI implementation](https://github.com/VirusTotal/yara/blob/v4.6.0/cli/yara.c), [YARA command line](https://yara.readthedocs.io/en/stable/commandline.html).

## 4. APIs, credentials and data leaving the workspace

**No paid account or API key is required for the core image or its local static scans.** Downloading vulnerability databases, signatures and pinned packages still needs outbound network access during explicit maintenance/build operations. A configured `offline` option is defense in depth, not an OS network isolation mechanism; enforce egress separately when a hard offline boundary is required.

| Integration | Authentication | What leaves the system | Placement and limit |
| --- | --- | --- | --- |
| Trivy databases/check bundles | Public downloads normally require no key; registry throttling may apply | Registry/CDN requests and connection metadata | Explicit update action; per-owner cache; no source upload |
| FreshClam | No paid API key | Signature/version/CDN requests | Explicit update action; preserve FreshClam backoff; no file upload |
| OSV API | Public API, no API key in the documented query endpoint | Package name, ecosystem, version or commit queried | Optional server-side enrichment; identifiers may reveal private dependencies |
| Socket | Organization-scoped API token | Selected manifests/lockfiles and related dependency metadata | Optional control-side integration; avoid source/build/reachability expansion by default |
| HIBP Pwned Passwords | No key | Five-character SHA-1 prefix, not the full password or hash | Hash locally; return only needed match information |
| HIBP account/domain intelligence | Subscription key and User-Agent | Full email for ordinary search, or six-character hash prefix on eligible plans; domain for verified-domain query | Optional trusted service; no credential in the sandbox |
| Dependabot/Renovate | Repository integration/bot token; private registry credentials if needed | Dependency metadata and registry queries | Dedicated update workflow outside scanner boxes |

Sources: [Trivy DB](https://trivy.dev/docs/latest/configuration/db/), [FreshClam](https://docs.clamav.net/manual/Usage/SignatureManagement.html), [OSV query API](https://google.github.io/osv.dev/post-v1-querybatch/), [Socket CLI privacy FAQ](https://docs.socket.dev/docs/socket-cli-faq), [HIBP API](https://haveibeenpwned.com/API/v3), [Dependabot private registries](https://docs.github.com/en/code-security/dependabot/working-with-dependabot/configuring-access-to-private-registries-for-dependabot), [Renovate security](https://docs.renovatebot.com/security-and-permissions/).

### HIBP scope and privacy

HIBP is not a general API-token leak search. Password range queries use `https://api.pwnedpasswords.com/range/{first5}` with `Add-Padding: true`; match the suffix locally, discard padding, and never query progressively as a password is typed. Full-email lookups disclose the email. Pro/High-RPM plans also offer `/api/v3/breachedaccount/range/{first6}`; match locally and discard unrelated results. Domain queries require verified control. Respect `429`/`Retry-After` and subscription limits. Password screening requires no key; email/domain calls require `hibp-api-key` and User-Agent. No-match results have corpus/visibility limitations. Breach/paste data carries attribution requirements; Pwned Passwords has separate terms. Keep passwords, full hashes, keys and returned unrelated identities out of model context and logs. [Official API and terms](https://haveibeenpwned.com/API/v3).

### Socket scope and privacy

Socket supplies dependency behavior and reputation signals, including install scripts, suspicious network activity, obfuscation and package-confusion risks. Signals require triage; they do not prove malware absence or malicious intent. Basic manifest scans need no application-source upload according to its CLI FAQ. [Capabilities](https://docs.socket.dev/docs/socket-for-github), [FAQ](https://docs.socket.dev/docs/socket-cli-faq), [alert handling](https://docs.socket.dev/docs/what-to-do-with-socket-alerts).

`socket scan create --report --json PATH` needs `full-scans:create`; the report stage also needs `full-scans:list` and `security-policy:read`. Creation consumes one quota unit, report consumes two. Current CLI documentation requires Node.js 22 or newer and supports `SOCKET_CLI_API_TOKEN` and the older `SOCKET_SECURITY_API_TOKEN`. Avoid `--auto-manifest`, `--dynamic-sbom-inference`, `--reach` and unreviewed `socket.json`: these can expand processing into build tools or application analysis. The scan configuration is discovered from the working directory and its ancestors. The optional bridge should expose fixed narrow queries, not a token-bearing unrestricted CLI. [Scan command](https://docs.socket.dev/docs/socket-scan), [CLI configuration](https://docs.socket.dev/docs/socket-cli).

## 5. Update bots, DAST and remediation

Dependabot is the smaller operational choice when GitHub already hosts the repository. Configure updates for Cargo, Docker and GitHub Actions, use a reviewed cadence, and assess security-update PRs with build/tests and vulnerability rechecks. Private registry secrets belong in Dependabot's supported secret store. Renovate provides more scheduling/grouping flexibility but adds a service and bot-token lifecycle; keep `allowScripts` and `allowPlugins` disabled unless a particular trusted workflow needs them. Neither bot validates the business safety of an upgrade. [Dependabot options](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference), [Renovate configuration](https://docs.renovatebot.com/self-hosted-configuration/).

Dynamic application testing is a separate target-aware capability. The native Boxoro web check is intentionally bounded to a local HTTP origin. Broader authenticated application testing requires an isolated application instance, test data, explicit target scope and an appropriate session/authorization model. Static scanners cannot test runtime access control or business logic.

OWASP ZAP is an appropriate optional dedicated image, rather than adding another browser/Java stack to every workspace. Its baseline script crawls for a short duration and runs passive checks; crawling still sends real requests. Full/API scans can perform active attacks and need a separately authorized disposable target. Resolve and record the current official image's real digest before deployment; a mutable `stable` tag is discovery information, not a pin. [ZAP baseline and official image usage](https://www.zaproxy.org/docs/docker/baseline-scan/), [ZAP full scan](https://www.zaproxy.org/docs/docker/full-scan/).

A practical remediation cycle is: reproduce the finding; identify the reachable vulnerable behavior or exposed credential; propose the smallest fix; run a scoped build/test in a fresh box; repeat the relevant scanner; compare evidence and residual coverage. Secret exposure requires revocation/rotation and investigation, not only deleting a string. Malware suspicion requires review and containment; do not run the suspicious sample to "confirm" it in an ordinary development box. Fixing a package-version match also requires checking compatibility and the actual resolved lockfile.

## 6. Licensing decisions

The image bundles the CE engine with **original Boxoro Semgrep rules**. Semgrep-maintained registry rules have separate Semgrep Rules License 1.0 restrictions, including internal-business-use conditions and a prohibition on using those rules in competing products/SaaS. An LGPL engine license does not grant permission to redistribute every registry ruleset. Third-party rule collections have their own licenses. [Official licensing page](https://docs.semgrep.dev/licensing).

The installer preserves bundled notices and downloads the pinned upstream license files. Redistribution of GPL/AGPL/LGPL software also requires the corresponding applicable source/notice obligations; copying license text alone is not a complete compliance process. ClamAV's bundled UnRAR code has additional conditions. Review licenses for container redistribution, modifications, linking and hosted service use, including the optional AGPL TruffleHog/Renovate choices. Do not assume a paid API's data can be republished without checking its terms. [ClamAV license](https://github.com/Cisco-Talos/clamav/blob/clamav-1.5.4/COPYING.txt), [bundled UnRAR conditions](https://github.com/Cisco-Talos/clamav/blob/clamav-1.5.4/libclamunrar/license.txt), [TruffleHog license](https://github.com/trufflesecurity/trufflehog/blob/v3.99.8/LICENSE), [Renovate source and license](https://github.com/renovatebot/renovate).

## 7. Verification record and remaining runtime work

The implementation includes an integrity test suite, deterministic Python-lock regeneration, a build script and a real container smoke script. Local checks verified archive extraction behavior, digest rejection, offline cache failures, platform rejection, Python syntax, shell syntax and exact lock regeneration. All six core native artifacts were subsequently downloaded and installed successfully through the verified installer. Semgrep 1.185.0 and its 66 active transitive runtime dependencies were installed successfully from the fully hashed wheel lock. A real Gitleaks runner scan detected a synthetic secret, produced a failing gate, and kept the synthetic value out of the normalized reports. The ClamAV Debian artifact was also downloaded, hash/size-verified and inspected: amd64, package version 1.5.4-1, expected `/usr/local` paths, and no declared external package dependencies or maintainer scripts. These are direct host checks, not a container or gVisor execution claim.

A bounded follow-up extracted ClamAV's runtime executables, shared libraries and vendor CVD certificate into scratch, without installing the package system-wide. With a private `LD_LIBRARY_PATH` and `--cvdcertsdir`, both version probes reported 1.5.4. The test exposed that plain `freshclam --version` exits 2 when the vendor's default configuration is absent; the Dockerfile and image smoke now use the verified `freshclam --config-file=/dev/null --version` form. A locally authored, **test-only `.ndb` signature** matched the synthetic EICAR fixture with exit 1; a harmless file returned 0, and an empty database returned 2. The runner's actual text parser normalized the match into one high-severity finding. This verifies loading and parser behavior, not production signature coverage or database resource requirements. The pinned YARA build was stopped at its prerequisite check because host `autoconf`, `autoreconf`, `automake` and `libtoolize` were unavailable; no dependencies were installed.

**No Docker daemon/CLI is available in the development environment used for this packaging work.** No container build, final image size, image digest, gVisor execution, production ClamAV signature load or complete image smoke result is claimed here. The script records actual identities and executes the smoke checks on a Docker-enabled host. The smoke uses uid 1000, read-only root, network disabled, temporary writable workspace, actual version commands, a synthetic secret detection/redaction check and a missing-database incomplete gate. Set `BOXORO_SMOKE_RUNTIME=runsc` to perform that check with gVisor on a configured worker.

Before rollout, build the selected target against the reviewed workspace base, run its smoke test, seed databases through the explicit update action, measure resource limits on representative repositories, and record the resulting freshness and coverage. A successfully built image is the beginning of operational verification; it is not a malware-free certificate for scanned code.
