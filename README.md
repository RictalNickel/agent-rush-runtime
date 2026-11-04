# Agent Rush

Give every agent a world: a native Rust controller, durable policy broker, private execution worker and an evidence-first web workspace.

**Current status: local implementation; not a completed hackathon submission.** Unit/API tests and browser flows run locally. Earlier bounded gVisor probes demonstrated code execution, filesystem/network containment, artifacts, timeout, output/storage limits and cleanup. The host failed the memory-limit probe: its Docker daemon has no memory/CPU/PID controllers and runsc uses `--ignore-cgroups`. The worker now refuses this host. Real Vultr inference, cloud deployment, private GitHub creation and full host qualification remain blocked by access/configuration prerequisites. See [verification](docs/VERIFICATION.md).

## Run the control UI locally

Requires Rust 1.90.0, a C compiler for bundled SQLite, and ordinary system CA certificates. The web application is served by Rust; no frontend build framework or Node runtime is required in production.

```sh
cargo build --locked
export AR_OPERATOR_PASSWORD='choose-a-local-password-of-at-least-16-characters'
export AR_SECURE_COOKIES=false   # loopback development only
cargo run --locked -- serve
```

The control server binds loopback port 8080 by default. An unconfigured instance supports login, a durable queue, cancellation and downloadable queued-job evidence. It does not fabricate model results or worker availability. Do not expose development HTTP publicly.

In this prepared workspace, activate Rust first:

```sh
export CARGO_HOME=/workspace/.toolchains/cargo
export RUSTUP_HOME=/workspace/.toolchains/rustup
export PATH="$CARGO_HOME/bin:$PATH"
cd /workspace/Hackathon
```

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

Chosen model: **GLM 5.3 Flash**. The exact Vultr API identifier and pricing still require catalog verification; no alternative model is selected. See [configuration choices](docs/CONFIGURATION.md) and the private credential-record location listed there.

- `AR_WORKER_URL`, `AR_WORKER_TOKEN`: private worker route and authentication.
- `AR_WORKER_CA_PATH`, `AR_WORKER_IDENTITY_PATH`: server trust and combined client certificate/private key PEM for mTLS.
- `VULTR_INFERENCE_API_KEY`, `AR_INFERENCE_MODEL`: server-only Vultr inference binding and selected model.
- `AR_INPUT_USD_PER_MILLION`, `AR_OUTPUT_USD_PER_MILLION`: internal provider-rate metadata filled by trusted setup, not user-selected prices. Blank/missing values disable paid inference; setup obtains the actual rates for GLM 5.3 Flash.
- `AR_PRICING_VERIFIED`: internal setup attestation, false by default. Set true only after checking the actual provider/model tariff and units; rate numbers alone do not activate calls.
- `AR_INFERENCE_BUDGET_USD`: persisted inference budget, default $5, hard maximum $25.
- `AR_DATA_DIR`, `AR_CAPACITY_MB`: trusted state directory and admission capacity; reserve node headroom when choosing capacity.

All agent roles use the fixed `https://api.vultrinference.com/v1/chat/completions` endpoint. No alternate model provider or local-shell tool exists. Logical subordinate contexts share task identity; they are not OS sandboxes. The controller permits four main turns and at most one bounded subordinate model request. Unresolved usage keeps its conservative cost reservation.

The Vultr infrastructure API key is used **only by trusted deployment tooling**, not the web server, agent contexts or worker tasks. `agent-rush vultr-plan --credential-file <owner-only-file>` performs read-only pricing/account inspection; it does not create machines. No actual prices have been verified yet.

Trusted setup now includes `vultr-inference-key` (reuse an existing serverless subscription key, private export only), `vultr-inference-model` (exact catalog selection, no guessed prices), and `vultr-manifest`/`vultr-track`/`vultr-teardown` (private exact-resource tracking and dry-run-first ownership-checked cleanup). These commands remain separate from the web/agent runtime. See [deployment](docs/DEPLOYMENT.md); live API access and cloud acceptance are still blocked.

## Delivery boundaries

The user's latest instructions require a private repository, Rust platform components, Agent Zero as reference only, a $300 ceiling and machines active only when needed. Keep at least $50 in reserve. No GPU, automatic scale-out or unattended demo is enabled. A stopped Vultr VM can still incur charges; delete newly created temporary test resources after preserving evidence and reproducible configuration.

Nested Docker, browser/desktop profiles, live memory expansion on an executing container, full Agent Zero plugin compatibility, cloud provisioning/automatic billing cutoff, an independently validated public deployment and a live demo recording are not delivered or advertised as supported. See the feature matrix and remaining work in docs/VERIFICATION.md.
