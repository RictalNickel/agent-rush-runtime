# Boxoro specialist agents

## Intent and authorization

Implement the twenty specialist agents proposed in the conversation plus Finance Lab, using the existing security agent as the integration pattern. The user explicitly requested research, full implementation, documentation and GitHub delivery. Routine architectural choices are delegated to the implementer; this document records those choices without requiring another design approval.

## Architecture

Use one versioned, validated catalog for specialist identity, tool ownership, fixed CLI recipes, typed parameters, risk declarations, image families and source references. A native `specialist` tool invokes a bounded Python runner in the existing owner-scoped workspace. The owner may select a specialist in agent configuration; selection changes prompt guidance, not the sandbox's permissions. Every specialist has an embedded skill and an operating guide. Existing security tools remain intact.

Image families share the existing workspace image and add isolated tool installations. A complete `suite` image combines portable tool packs; smaller family targets are also supported. Native Android and hardware profilers require dedicated hosts and are represented as host integrations, never falsely reported installed. Workers retain their current one-image-per-worker configuration; operators route to appropriately configured workers. No implicit scheduler is invented.

Finance has a separate trusted OpenBB service. It discovers the installed V5 OpenAPI schema, allows provider data operations and analytical operations, keeps credentials outside workspaces, and exposes a small compatible MCP surface. A local finance CLI performs deterministic research calculations without brokerage/trade execution. Provider availability and data entitlements remain explicit.

## Approaches considered

1. Independent agent applications and images: high duplication and integration drift.
2. One unbounded shell/tool installation: broad reach but unreliable dependencies and no explicit coverage.
3. Shared runtime, reviewed catalog, isolated package environments, specialist skills and image families: selected by the implementer for maintainability and auditable coverage.

## Binding requirements

- All 21 new specialist identities must be addressable through native Boxoro tools and skills.
- Every tool has a primary source, license note, installation method and explicit availability classification.
- Registered commands use fixed executable paths and argv templates, never model-supplied shells, executable paths, environment variables or arbitrary CLI arguments.
- Typed parameters reject unknown keys, NULs, oversized input, unsafe path escapes and unsupported values.
- Tools requiring project-code execution, network access or workspace mutation require an explicit request declaration. These declarations are not independent authorization or an OS security boundary.
- Remote mutations, production changes, money movement and trades are not performed by registered default commands.
- Commands have bounded time/output and process-group cleanup. Missing tools, failed probes, unsupported schemas and limits cannot result in a passing gate.
- Reports include source/time/tool evidence and distinguish findings from incomplete coverage.
- Versions and resolved package dependencies are locked; recipes and host integration dependencies are documented even where the environment cannot execute them.
- No provider credentials are embedded in images, source, reports or examples.
- OpenBB integration uses installed V5 resources and documented extension mechanisms, not hard-coded obsolete V4 endpoints.
- Fresh automated tests plus available real-tool smoke tests precede completion claims. Image builds, paid provider calls and host qualification are never claimed without evidence.

## Runtime contract (v1)

Request to `/opt/box/bin/specialist`:

```json
{"action":"run","agent":"testforge","operation":"pytest-test","path":"app","parameters":{},"allow_project_code":true,"allow_network":false,"allow_mutation":true,"timeout_seconds":120}
```

Actions: `list`, `describe`, `doctor`, `run`, `workflow`. Optional fields: `agent`, `operation`, `workflow`, `path`, `parameters`, three effect declarations and `timeout_seconds` (10..540). `list` returns an index. `describe` returns one full agent and its operations. `doctor` validates installation and prerequisites without installing. `run` executes one member operation. `workflow` executes a named bounded sequence and stops on failed/incomplete gates. Parameters are shared only when explicitly referenced by a step. Native Rust exposes `operation` because the outer envelope already uses `tool`.

Result: `schema_version`, `action`, `agent`, `gate` (`pass`, `fail`, `incomplete`), `checks`, `reports`, `limitations`. Each check records operation/tool, status, exit code and duration. Native reports are local artifacts, not implicitly safe to publish. Summary output omits full stdout/stderr; capped output is available in private per-run report files.

## Catalog contract (v1)

`deploy/agents/catalog.json` contains objects `agents`, `tools`, `operations`, `connectors` indexed by stable lowercase hyphenated IDs, plus `schema_version: 1`.

Agent: `name`, `description`, `family`, `skill` (Markdown body), `tools` (IDs), `workflows` (mapping name to operation ID list), `connectors` (IDs), `resources` (memory_mb/disk_mb), `limitations` (strings).

Tool: `name`, `description`, `homepage`, `sources` (URLs), `license`, `family`, `install` (kind `python`, `npm`, `binary`, `apt`, `builtin`, `host`, or `external`; package, version, executable as applicable), `probe` (argv suffix), `availability` (`image`, `optional`, `host`, `external`). Installed command root is `/opt/box/agents/bin/<tool-id>`; installers create trusted wrappers/symlinks there. Root owns install lock generation, catalog author supplies package coordinates and accurate version constraints.

Operation: `tool` (ID), `description`, `argv` (suffix tokens only; executable comes from tool ID), `parameters` (JSON-schema-like map of type, enum, minimum, maximum, maxLength, default, required and format), `effects` (subset `project_code`, `network`, `mutation`), `success_codes`, `finding_codes`, `timeout_seconds`, `notes`. Templates support `{path}`, `{artifacts}`, and `{parameter_name}`; all parameter placeholders use validated scalar strings. Parameter formats: `path` (existing workspace path), `output_path` (workspace-contained), `loopback_url`, `https_url`, `identifier`. Paths are resolved before interpolation; unknown placeholders rejected. A command cannot supply its executable.

Connector: `name`, `vendor`, `url`, `transport`, `status`, `auth`, `allowed_tools_notes`, `agents`, `sources`, `limitations`. Only verified live HTTP MCP examples may be operational config; reference entries do not pretend to be deployed connections.

## Validation focus

Cross-profile operation misuse; path and argument escape; configuration that executes project code; output/timeout/cancellation; dependency incompatibilities; stale provider namespaces; financial unit/frequency/annualization errors; secret leakage through provider metadata; incorrect pass for missing coverage; worker image mismatch.
