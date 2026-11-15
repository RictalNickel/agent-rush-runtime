# MobileLab

Inspect Android packages and prepare device-qualified Android/iOS test workflows.

Profile ID: `mobilelab`. Preferred family: `mobile`. Planning allowance: **4096 MiB RAM / 8192 MiB free disk** for ordinary bounded jobs. These are sizing estimates, not measured image sizes or hard guarantees.

See [runtime contract](../SPECIALIST_RUNTIME.md) for request validation, private reports, effect declarations and failure handling, and [tool research](../AGENT_TOOL_RESEARCH.md) for installation, license and connector decisions.

## Operating skill

# MobileLab

Separate portable APK/source inspection from tests that require a real device, emulator or Apple host. Make platform readiness visible before claiming a result.

## Working method

Identify app/package version, platform, minimum SDK, signing expectations and permitted test target. On the portable worker inspect APK identity, manifest/resources and reviewed source rules without installing or running the app. For behavior/performance testing qualify adb serial/authorization, emulator acceleration, SDK images and Appium/Maestro drivers on the host. iOS builds/simulators require macOS and the selected Xcode; do not imply Linux support. Capture lifecycle, permissions, offline, background and accessibility cases on the actual supported target when available.

## Deliverable

APK/static evidence, source findings, an explicit host/device readiness matrix, reproducible test flows and device-qualified screenshots/performance results only when actually executed.

## Runtime discipline

Use `specialist` with this agent ID. Start with `describe` and `doctor`; select only operations listed for this profile. The `image` label means an image build recipe exists. A successful live doctor probe and the installed lock are the evidence that a command is available. Optional, host and external entries are not silently installed or connected.

Use the exact input paths and typed parameters required by the operation. Declare project-code execution, network and workspace mutation only when the task authorizes those effects. These declarations are request checks, not an operating-system security boundary. Project tests, plugins and configs can perform additional actions; review their scope. Do not invent free-form shell, executable, environment or CLI-argument fields.

Interpret a passing inventory/report operation as successful evidence capture, not as approval of everything it describes. Preserve failed/incomplete checks, skipped coverage, tool versions, timestamps, source paths and artifacts. Private logs can retain sensitive content; review before sharing. Never convert an unavailable tool, empty test suite, missing project dependency, timeout or missing external account into a passing quality claim.

Installed CLI environments are isolated. A globally installed pytest/Vitest or other wrapper may not see the project's virtual environment or node_modules. When the reviewed project environment is required, use the existing ordinary execution tool with explicit task authorization; there is no automatic project-executable bypass in specialist requests. Do not repair this by globally upgrading unrelated tool dependencies.

## Tool coverage

The catalog is the source of truth. `image` identifies a build recipe, not an assertion that the current worker contains the tool. An optional library has no pretend CLI probe. Shared tools can expose additional fixed utilities; workflows below select the intended specialist task.

| Tool | Availability | Install coordinate | License | Primary source |
| --- | --- | --- | --- | --- |
| Appium (`appium`) | optional | `npm: appium` | Apache-2.0 | [upstream](https://github.com/appium/appium) |
| Android Debug Bridge (`adb`) | image | `apt: adb` | Apache-2.0 | [upstream](https://android.googlesource.com/platform/packages/modules/adb/) |
| Android Emulator (`android-emulator`) | host | `host: Android Emulator` | Android SDK terms and bundled component licenses | [upstream](https://developer.android.com/studio/run/emulator) |
| Android APK Analyzer (`apkanalyzer`) | host | `host: Android APK Analyzer` | Android SDK terms and Apache-2.0 source components | [upstream](https://developer.android.com/tools/apkanalyzer) |
| Apktool (`apktool`) | image | `apt: apktool` | Apache-2.0 | [upstream](https://github.com/iBotPeaches/Apktool) |
| Androguard (`androguard`) | image | `python: androguard ==4.1.4` | Apache-2.0 | [upstream](https://github.com/androguard/androguard/tree/v4.1.4) |
| JADX (`jadx`) | host | `host: JADX` | Apache-2.0 | [upstream](https://github.com/skylot/jadx) |
| Maestro (`maestro`) | host | `host: Maestro` | Apache-2.0 | [upstream](https://github.com/mobile-dev-inc/Maestro) |
| detekt (`detekt`) | host | `host: detekt` | Apache-2.0 | [upstream](https://github.com/detekt/detekt) |
| ktlint (`ktlint`) | host | `host: ktlint` | MIT | [upstream](https://github.com/pinterest/ktlint) |
| SwiftLint (`swiftlint`) | host | `host: SwiftLint` | MIT | [upstream](https://github.com/realm/SwiftLint) |
| Xcode build/test tools (`xcodebuild`) | host | `host: Xcode build/test tools` | Apple Xcode and Apple SDK agreements | [upstream](https://developer.apple.com/xcode/) |
| Playwright Test (`playwright`) | image | `npm: @playwright/test 1.64.1` | Apache-2.0 | [upstream](https://github.com/microsoft/playwright) |
| Boxoro analysis (`boxoro-analysis`) | image | `builtin: boxoro 1` | Project license; see repository LICENSE | `repository:deploy/agents/boxoro_agents/builtins.py` |
| Semgrep CE (`semgrep`) | image | `python: semgrep` | LGPL-2.1-or-later | [upstream](https://github.com/semgrep/semgrep) |

## Recommended workflows

A workflow stops at a failed or incomplete step. It preserves the result and does not claim that later skipped checks ran. Parameters are supplied only to steps that reference them.

| Workflow | Ordered operations |
| --- | --- |
| `apk-static` | `android-apk-id` → `android-manifest` → `apktool-decode` |
| `source-audit` | `semgrep-scan` |
| `mobile-web` | `playwright-test` |

### Discovery and readiness

```json
{
  "action": "describe",
  "agent": "mobilelab"
}
```

```json
{
  "action": "doctor",
  "agent": "mobilelab"
}
```

A run request selects an operation, a workspace path and only its declared parameters. Set effect declarations only when the actual task authorizes them. `allow_project_code`, `allow_network` and `allow_mutation` are explicit request checks; the existing worker sandbox remains the operating-system boundary.

## Fixed operations

Executable paths come from trusted tool IDs under `/opt/box/agents/bin/`. Suffixes below are fixed templates. A parameter never becomes a model-selected executable, shell command or arbitrary argument array. `{artifacts}` is the private report directory created by the runtime.

### `android-apk-id`

Read package and version identity from an authorized APK.

Command: `/opt/box/agents/bin/androguard apkid {input}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Published Androguard 4.1.4 CLI. Static metadata is not proof of signature integrity, device compatibility or app security.

### `android-manifest`

Decode the Android manifest from an authorized APK.

Command: `/opt/box/agents/bin/androguard axml {input}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Offline static parse only. App behavior, dynamic permissions and native libraries require a separate device-qualified test plan.

### `apktool-decode`

Decode an authorized APK into an isolated artifact directory.

Command: `/opt/box/agents/bin/apktool decode --no-src --output {artifacts}/apk-decoded {input}`

Effects: mutation. Tool timeout: 180 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `input` | string; path; required |

Static manifest/resource inspection. Does not install, run, patch, sign or upload an APK; malformed archives remain untrusted parser inputs.

### `playwright-test`

Run the project browser suite with one worker and JSON output.

Command: `/opt/box/agents/bin/playwright test --workers=1 --reporter=json --output={artifacts}/playwright`

Effects: project_code, network, mutation. Tool timeout: 300 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

The runtime qualifies the JSON report: at least one test must execute, and missing evidence or runtime errors are incomplete. Project configuration and tests execute code; passing tests cover only the declared scenarios, not every browser or interaction.

### `semgrep-scan`

Run community static analysis with a supplied local rule file.

Command: `/opt/box/agents/bin/semgrep scan --config {rules} --metrics off --disable-version-check --json --error {path}`

Effects: none declared. Tool timeout: 120 seconds. Success codes: `[0]`; finding codes: `[1]`. Other exits, unavailable tools, truncated output and timeouts remain incomplete.

| Parameter | Contract |
| --- | --- |
| `rules` | string; path; required |

Rules are explicitly supplied local YAML; no registry auto configuration, hosted engine or telemetry. Scanner internal errors remain incomplete; rule matches fail.

### Additional fixed utilities

| Operation | Purpose | Effects |
| --- | --- | --- |
| `inventory` | Inventory bounded regular files with sizes and SHA-256 hashes. | none declared |
| `json-check` | Validate strict JSON syntax and duplicate-key policy. | none declared |
| `csv-profile` | Profile UTF-8 comma CSV without emitting cell contents. | none declared |
| `locale-check` | Check locale key and common placeholder parity. | none declared |
| `rag-evaluate` | Compute binary-judgment precision, recall, MRR and nDCG. | none declared |
| `privacy-scan` | Report redacted heuristic sensitive-data pattern counts. | none declared |
| `knowledge-audit` | Check local Markdown links/headings and duplicate file hashes. | none declared |
| `agent-scaffold` | Create a new reviewable Python agent package. | mutation |
| `playwright-screenshot` | Capture a full-page Chromium screenshot of a loopback app. | project_code, network, mutation |

Use `describe` for each utility’s exact parameters, exit interpretation and notes. A descriptive report such as complexity, an SBOM or Lighthouse metrics does not automatically enforce a quality threshold.

## Integrations and prerequisites

### GitHub official MCP (`github-mcp`)

Status: **reference-not-connected**. Transport: `streamable-http`.

User-authorized OAuth/PAT supplied through the existing MCP integration; never in an image.

Start with X-MCP-Readonly:true and explicit repos/pull_requests/actions toolsets. Sending messages, opening PRs, merging, releasing and other writes need separate explicit user authorization.

Primary documentation: [source 1](https://github.com/github/github-mcp-server), [source 2](https://github.com/github/github-mcp-server/blob/main/docs/server-configuration.md).

### Playwright MCP (`playwright-mcp`)

Status: **reference-not-connected**. Transport: `stdio`.

No cloud token required for a local disposable browser; authenticated browser sessions remain separately authorized.

Reviewed @playwright/mcp installation; only scoped browser actions. Do not expose arbitrary JavaScript or unrestricted browser profiles to untrusted clients.

Primary documentation: [source 1](https://github.com/microsoft/playwright-mcp), [source 2](https://playwright.dev/docs/getting-started-mcp).

### Tool-specific qualifications

- **Appium:** Appium alone has no device drivers. Android/iOS drivers, host connections and device permissions must be installed and qualified separately.
- **Android Debug Bridge:** The CLI is portable and installed; a device/emulator connection is host-dependent. No device manipulation or claim of emulator readiness follows from a successful version probe.
- **Android Emulator:** Requires accepted SDK/system-image terms, KVM or equivalent acceleration, typically >=8 GiB RAM and 20 GiB disk; cannot be assumed inside an unprivileged worker.
- **Android APK Analyzer:** Requires Android SDK command-line tools and compatible JDK.
- **Apktool:** JRE required by Ubuntu package; this does not install, launch or modify a device app.
- **Androguard:** Pin published stable 4.1.4. Upstream main is currently a materially different unpublished 5.x design with Rust and Git dependencies; do not install main implicitly.
- **JADX:** Requires a qualified JVM and explicit output directory. Source reconstruction is incomplete for obfuscated/native code.
- **Maestro:** Android requires an authorized adb target; iOS simulator requires macOS and Xcode. Maestro Cloud is a separate service.
- **detekt:** Run the project-pinned Gradle plugin or locked CLI JAR; Gradle build logic executes project code.
- **ktlint:** Run a locked JVM artifact or project Gradle plugin; auto-correction is disabled in qualification recipes.
- **SwiftLint:** Requires a compatible Swift toolchain; typical iOS validation runs on macOS.
- **Xcode build/test tools:** Requires a licensed macOS host, selected Xcode version, simulator/device target and signing policy. No iOS capability is claimed on Linux.
- **Playwright Test:** Pinned to match the 1.64.1-noble browser image revision. Matching browser revision and OS libraries are required. Project configs/tests execute JavaScript.
- **Semgrep CE:** Community engine only; commercial engines, registry rule licenses and hosted services are separate. License expression follows the resolved 1.185.0 PyPI metadata. Resolved 1.185.0 pins mcp 1.29.0 and OpenTelemetry 1.37.x with known moderate advisories. The registered operation is local scan with metrics/version checks disabled; no MCP client/server or telemetry endpoint is exposed. This limited use does not erase the dependency findings; see SPECIALIST_DEPENDENCIES.md.

## Limits and interpretation

- Installed adb CLI does not provide a connected device or working emulator.
- Android emulator needs host acceleration and licensed system images; gVisor capability is not assumed.
- iOS requires macOS/Xcode; mobile browser emulation is not native app validation.

All version claims come from the resolved lock and live doctor output. This guide does not assert that an image build, paid provider call, device test or production connection ran in this checkout.
