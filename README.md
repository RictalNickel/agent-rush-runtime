# Agent Rush

Give every agent a world: a native Rust controller, durable policy broker, private execution worker and an evidence-first web workspace.

**Status (2026-11-05): live on Vultr.** A two-VM deployment (Caddy HTTPS control plane plus a private gVisor worker reached over mTLS) runs real agent jobs at https://144-202-59-108.sslip.io. The worker passes all nine real containment qualification checks; jobs, a four-job load test and containment probes have receipts in `docs/evidence/live/`. See [live deployment](docs/LIVE_DEPLOYMENT.md) for what is verified and what is still open.

Agent Rush is an original Rust system. Its agent loop was designed with [Agent Zero](https://github.com/agent0ai/agent-zero) (MIT) as a reference; no Agent Zero code, prompts or tool envelopes are reused. See [design lineage](docs/reference/DESIGN_LINEAGE.md) and [third-party notices](THIRD_PARTY_NOTICES.md).

## Run the control UI locally

Requires Rust 1.90.0, a C compiler for bundled SQLite, and ordinary system CA certificates. The web application is served by Rust; no frontend build framework or Node runtime is required in production.

```sh
cargo build --locked
export AR_OPERATOR_PASSWORD='choose-a-local-password-of-at-least-16-characters'
export AR_SECURE_COOKIES=false   # loopback development only
cargo run --locked -- serve
```

The control server binds loopback port 8080 by default. An unconfigured instance supports login, a durable queue, cancellation and downloadable queued-job evidence. It does not fabricate model results or worker availability. Do not expose development HTTP publicly.

## Tests

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all --check
```

Real container tests are intentionally explicit:

```sh
cargo test --locked --test sandbox -- --ignored --nocapture --test-threads=1
```

Run these on a properly configured Linux worker with host cgroups, Docker and runsc, and the pinned image already pulled. They are **not passing on this managed host**; the new preflight refuses its missing controllers. Never disable that preflight or switch to privileged/runc execution to make the tests green.

For browser checks, build the debug binary and leave port 8080 free. Playwright starts/stops its own Rust server with a fresh temporary database, a synthetic local login, and inference disabled:

```sh
cd qa
npm ci
npx playwright test --config playwright.config.cjs
```

Tests use the installed `/usr/bin/chromium`; adjust that explicit test-only path on another workstation. Browser plugin was unavailable, so regular Playwright was used.

## Execute Rust tasks on a qualified worker

```sh
docker pull rust:1.90.0-bookworm@sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f
cargo run --locked -- doctor
cargo run --locked -- sandbox examples/revenue.rs --timeout 60 --receipt runtime/revenue.json
```

Generated tasks use the Rust standard library, no dependency downloads. They run only inside the fixed gVisor profile. Credentials, host paths and the host Docker socket never enter the task. Artifacts must be bounded regular files in `/work/artifacts`; `result.json` must contain a JSON object. These checks prove execution and structural output validity, not arbitrary semantic correctness.

## Connect a worker and real inference

The worker requires a random `AR_WORKER_TOKEN` of at least 32 characters. Start it using `agent-rush worker`; it binds loopback port 8090. Remote transport requires a private network, mTLS ingress and source-limited firewall rules. See [deployment](docs/DEPLOYMENT.md).

Control settings:

- `AR_INFERENCE_PROVIDER`: `anthropic` (Claude agent team) or `vultr` (Vultr Serverless Inference, the default). Every role uses the selected provider; model output never chooses it. See [the agent team](docs/AGENT_TEAM.md).
- `ANTHROPIC_API_KEY`: server-only key for the `anthropic` provider. Calls are priced from Anthropic's published list prices.
- `AR_WORKER_URL`, `AR_WORKER_TOKEN`: private worker route and authentication.
- `AR_WORKER_CA_PATH`, `AR_WORKER_IDENTITY_PATH`: server trust and combined client certificate/private key PEM for mTLS.
- `VULTR_INFERENCE_API_KEY`, `AR_INFERENCE_MODEL`: server-only Vultr inference binding. The catalog lists `glm-5.3-flash`.
- `AR_INPUT_USD_PER_MILLION`, `AR_OUTPUT_USD_PER_MILLION`, `AR_PRICING_VERIFIED`: Vultr tariff metadata. Blank or unverified values keep the `vultr` provider disabled rather than assuming a zero price.
- `AR_INFERENCE_BUDGET_USD`: persisted inference budget, default $5, hard maximum $25. Each call reserves its worst case before it is sent and settles to actual usage.
- `AR_DATA_DIR`, `AR_CAPACITY_MB`: trusted state directory and admission capacity; reserve node headroom when choosing capacity.

**Hackathon note:** the Blast Radius Zero track requires Vultr Serverless Inference for every agent LLM call. The live demo currently runs on the `anthropic` provider for development; switch to `AR_INFERENCE_PROVIDER=vultr` with verified GLM 5.3 Flash pricing before submitting.

Logical subordinate contexts share task identity; they are not OS sandboxes. Unresolved usage keeps its conservative cost reservation.

The Vultr infrastructure API key is used **only by trusted deployment tooling**, not the web server, agent contexts or worker tasks. `agent-rush vultr-plan --credential-file <owner-only-file>` performs read-only pricing/account inspection; it does not create machines. The live VMs were provisioned and installed with `deploy/bootstrap-worker.sh`, `deploy/install-worker.sh` and `deploy/install-control.sh`.

Trusted setup now includes `vultr-inference-key` (reuse an existing serverless subscription key, private export only), `vultr-inference-model` (exact catalog selection, no guessed prices), and `vultr-manifest`/`vultr-track`/`vultr-teardown` (private exact-resource tracking and dry-run-first ownership-checked cleanup). These commands remain separate from the web/agent runtime and were not used for the live deployment. See [deployment](docs/DEPLOYMENT.md).

## Delivery boundaries

The user's latest instructions require a private repository, Rust platform components, Agent Zero as reference only, a $300 ceiling and machines active only when needed. Keep at least $50 in reserve. No GPU, automatic scale-out or unattended demo is enabled. A stopped Vultr VM can still incur charges; delete newly created temporary test resources after preserving evidence and reproducible configuration.

Nested Docker, browser/desktop profiles, live memory expansion on an executing container, full Agent Zero plugin compatibility, automatic billing cutoff and a live demo recording are not delivered or advertised as supported. A live two-VM deployment is running; see [live deployment](docs/LIVE_DEPLOYMENT.md). See the feature matrix and remaining work in docs/VERIFICATION.md.


How the agent loop relates to Agent Zero, and where it deliberately differs, is in [design lineage](docs/reference/DESIGN_LINEAGE.md): typed team roles with enforced allowlists, a reviewer gate, receipt-gated answers, persisted context exports, bounded owner memory, shared call limits and admitted between-attempt RAM growth. All production model calls go through one trusted inference gateway; generated code executes only on the private worker.

Run `agent-rush qualify --output <new-private-report.json>` on a candidate worker before enabling it, then set `AR_QUALIFICATION_REPORT` to that report. The command runs fixed real containment/resource probes and rejects unsupported hosts before executing code. Reports are bound to the exact host/image and expire after two hours; the deployed worker requalifies every 45 minutes and adopts each new passing report without a restart. The live Vultr worker passes all nine checks. See [image packaging](docs/IMAGES.md) for locked image inputs and the native control image build/export.


Independent tracked-resource cleanup is implemented by `vultr-watch` ([operations and limits](docs/CLEANUP_WATCHDOG.md)). It previews by default; an executing watcher authenticates the account scope, watches the fixed deadline, persists progress, and closes tracking before cleanup. Run it only on an appropriate trusted management runner. It has not been qualified against live Vultr and does not implement automatic VM creation or recover unknown creation requests.
