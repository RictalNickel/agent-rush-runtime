# Configuration

Vultr-Placeholder-Test123 reads its settings from environment variables. The defaults below come from the code: `src/config.rs`, `src/main.rs`, `src/boxes.rs`, `src/session.rs`, `src/worker.rs`, `src/mcp_client.rs` and `voice/server.py`, and from the deploy scripts where a variable is read there. The installers write values into `/etc/agent-rush/control.env` and `/etc/agent-rush/worker.env` (mode 0600). Templates are in `deploy/control.env.example` and `deploy/worker.env.example`. Installers never print secrets.

A variable marked "none" has no default and is unset unless you set it.

## Control plane

| Variable | Default | Meaning |
| --- | --- | --- |
| `AR_BIND` | `127.0.0.1:8080` | Address the web and API server listens on |
| `AR_DATA_DIR` | `runtime` | State directory, with the SQLite ledger `control.db` |
| `AR_OPERATOR_PASSWORD` | none | Operator sign-in password. Sign-in is refused below 16 characters |
| `AR_SECURE_COOKIES` | `true` | Adds the `Secure` flag to session cookies. Only the exact value `false` turns it off, for loopback development |
| `AR_CAPACITY_MB` | `2048` | Memory the control plane may admit for one-shot jobs. Leave headroom for the host |
| `AR_INFERENCE_PROVIDER` | `vultr` | `vultr` (Vultr Serverless Inference) or `anthropic` (development). Any other value stops startup |
| `VULTR_INFERENCE_API_KEY` | none | Vultr inference key, used by the control plane's inference gateway. Keep it out of `worker.env` |
| `AR_INFERENCE_MODEL` | none | Vultr catalog model ID. The public instance uses `glm-5.3-flash`. Vultr inference stays disabled until this, both tariffs and `AR_PRICING_VERIFIED=true` are set |
| `AR_INPUT_USD_PER_MILLION`, `AR_OUTPUT_USD_PER_MILLION` | none | Model tariffs in USD per million tokens. Blank means unknown, never zero |
| `AR_INFERENCE_MODELS` | none | Vultr only. Models a session may choose, as comma-separated `id:input:output` (USD per million tokens), for example `glm-5.3-flash:0.10:0.35,glm-5.3:0.75:3.00`. `AR_INFERENCE_MODEL` must be listed and stays the default; its listed prices become the default tariff, and an `AR_*_USD_PER_MILLION` value that differs stops startup. Up to 16 models, each with two prices. Unset: every session runs `AR_INFERENCE_MODEL`, as before. `AR_PRICING_VERIFIED=true` covers every listed price. A model removed from the list later fails the next call of a session that runs on it |
| `AR_PRICING_VERIFIED` | `false` | Set to `true` only after the tariffs are checked against the provider |
| `AR_INFERENCE_BUDGET_USD` | `5` | Total model spend the ledger allows. Must be between 0 and 25 |
| `ANTHROPIC_API_KEY` | none | Key for `AR_INFERENCE_PROVIDER=anthropic`. Calls are priced from Anthropic's list prices |
| `AR_WORKER_URL` | none | Worker address: `https://...`, or `http://` on loopback only. Without it, session routes return 503 |
| `AR_WORKER_TOKEN` | none | Worker bearer token, at least 32 characters. Must match the worker's token |
| `AR_WORKER_CA_PATH` | none | CA certificate that signs the worker's TLS certificate |
| `AR_WORKER_IDENTITY_PATH` | none | Client certificate and private key, as one PEM file, that the control plane presents to the worker |
| `AR_MCP_SERVERS_FILE` | none | JSON file of remote MCP servers the agent may use. See [MCP servers file](#mcp-servers-file) |
| `AR_VOICE_URL` | none | Voice service base URL, `http` or `https`, with no credentials. Without it, `/v1/voice/*` returns 404 |
| `RUST_LOG` | `agent_rush=info` | Log filter for the tracing subscriber |
| `AR_SESSION_MAX_OPEN` | `20` | Sessions one owner may have open at once (1–1000) |
| `AR_SESSION_MAX_STREAMS` | `8` | Live event streams one owner may hold at once (1–1000) |
| `AR_SESSION_MAX_LONG_REQUESTS` | `8` | Direct exec and file calls, and MCP waits, one owner may have in flight (1–1000) |
| `AR_SESSION_MAX_BOX_CALLS` | `16` | Box exec and file calls in flight across all sessions (1–1000). Raise it with the worker's `AR_BOX_MAX` |
| `AR_SESSION_BOX_MB` | `1024` | Memory a session box starts with (256–65536, at most the worker's `AR_BOX_CEILING_MB`). Each box reserves this much of the worker's capacity, and grows from it under load |
| `AR_SESSION_TURN_USD`, `AR_SESSION_USD` | `0.50`, `5.00` | Model spend one turn, and one session, may reach before the next model call is refused. The turn cap must not exceed the session cap |

Fixed in code and not settable: per-turn limits (30 minutes, $0.50 spend; there is no step count), per-session spend ($5), and the per-owner and box call caps listed in [API.md](API.md#limits). See [SECURITY.md](SECURITY.md#known-limits).

## Worker

| Variable | Default | Meaning |
| --- | --- | --- |
| `AR_DATA_DIR` | `runtime` | State directory for the worker. The installer uses `/var/lib/agent-rush/worker` |
| `AR_WORKER_TOKEN` | none | Bearer token every worker route requires, at least 32 characters |
| `AR_CAPACITY_MB` | `2048` | Memory the worker may admit. It is clamped to the host's memory less a reserve (a quarter of the host, at most 2 GiB plus 1/16: 2 GiB of 8, 4 GiB of 32, 6 GiB of 64). The example file sets `6144` |
| `AR_QUALIFICATION_REPORT` | none | Job qualification report. The worker refuses to start without it and refuses work once it is more than two hours old |
| `AR_SANDBOX_IMAGE` | `rust:1.90.0-bookworm@sha256:3914072c…` (the `IMAGE` constant in `src/worker.rs`) | Pinned image for one-shot runs |
| `AR_BOX_IMAGE` | none | Workspace image reference for session boxes. Without it the worker serves one-shot runs only. Both `deploy/install-worker.sh` and `scripts/install.sh` (worker role) set it to the digest of the image they build |
| `AR_BOX_NETWORK` | `ar-box` | Docker network for boxes. Its egress rules and host input guard come from `deploy/box-network.sh` |
| `AR_BOX_QUALIFICATION_REPORT` | `/var/lib/agent-rush/box-qualification.json` | Box qualification report. Box creation and restarts are refused unless it is fresh and passing for this host, boot, image and network |
| `AR_BOX_IDLE_SECONDS` | `1800` | Idle seconds before a box is parked: its container is removed and its `/workspace` kept. The next call restarts it |
| `AR_BOX_RETENTION_SECONDS` | `604800` (7 days) | Seconds a parked workspace is kept without a call. Its files are deleted then, or when the session closes. Must be at least `AR_BOX_IDLE_SECONDS` |
| `AR_BOX_MAX` | `8` | Boxes the worker runs at once (1–200), on top of the `AR_CAPACITY_MB` memory check. Size it to the host: the benchmark in [docs/BENCHMARKS.md](BENCHMARKS.md) shows what 4, 8 and 16 vCPUs sustain. Each session box also reserves `AR_SESSION_BOX_MB` of `AR_CAPACITY_MB` |
| `AR_BOX_CEILING_MB` | `4096` | Largest memory a box may grow to. Must be between 1024 and 65536 |
| `AR_BOX_DISK_MB` | `4096` | Hard size of each box's `/workspace`, in MiB. Must be between 256 and 262144. Creating a box needs twice this plus 2 GiB free, after the headroom existing workspaces may still grow into |
| `AR_BOX_GUARD_MARKER` | `/run/agent-rush/box-network.env` | Marker that `deploy/box-network.sh` writes once the egress rules and host input guard are verified, recording the boot ID. Box starts are refused without a marker for the current boot. It lives in `/run`, so a reboot clears it until the root unit reruns |
| `AR_BOX_PRIVATE_PROBES` | empty in the example; `deploy/install-worker.sh` sets the control VM's `ip:22` | Comma-separated private `ip:port` endpoints that the host reaches and boxes must not. Read by `deploy/requalify-box.sh`, which passes each one as `--private-probe`, not by the binary |

The worker's `--bind` default is `127.0.0.1:8090`. It is a command-line flag, not an environment variable.

### Box network overrides

`deploy/box-network.sh` reads these to build a separate, scratch box network, which the qualification run used. Leave them unset on a live worker.

| Variable | Default | Meaning |
| --- | --- | --- |
| `AR_BOX_NET` | `ar-box` | Docker network name. Set `AR_BOX_NETWORK` to the same value on the worker |
| `AR_BOX_SUBNET` | `172.30.0.0/24` | Subnet of the box network. The script's rules match this subnet only |
| `AR_BOX_GATEWAY` | `172.30.0.1` | Gateway address of the box network, which is an address on the host |
| `AR_BOX_BRIDGE` | `ar-box0` | Linux bridge name |
| `AR_BOX_TAG` | `ar-box-egress` | Comment on every rule, so the script changes only its own rules |
| `AR_BOX_MARKER` | `/run/agent-rush/box-network.env` | Marker file. Set `AR_BOX_GUARD_MARKER` to the same path on the worker |

## Installers and setup

These are read by `scripts/install.sh` and `scripts/setup.sh`, not by the Rust binary.

| Variable | Default | Meaning |
| --- | --- | --- |
| `AGENT_RUSH_BINARY` | none | Prebuilt `agent-rush` binary to install instead of building one |
| `AR_SKIP_TESTS` | `0` | Set to `1` to skip `cargo test` after a source build |
| `AR_PUBLIC_IP` | detected | Public IPv4 for the `sslip.io` name with `tls` |
| `AR_WORKER_PRIVATE_IP` | none | Role `control`: the worker's private address |
| `AR_SETUP_PROVIDER` | `vultr` | Written by `setup.sh`: inference provider |
| `AR_SETUP_INFERENCE_KEY` | none | Written by `setup.sh`: inference key, stored in the 0600 env file |
| `AR_SETUP_ADMIN_PASSWORD` | none | Written by `setup.sh`: operator password |

## Voice service

Read by `voice/server.py`. The service listens on `127.0.0.1:8790` and has no authentication. See [VOICE.md](VOICE.md).

| Variable | Default | Meaning |
| --- | --- | --- |
| `VOICE_REF_WAV` | `voice/ref.wav` beside `server.py` | Reference clip for speech output |
| `VOICE_DEVICE` | `cpu` | `cpu` or `cuda` for LuxTTS |
| `VOICE_THREADS` | `4` | CPU threads for LuxTTS |
| `VOICE_STT_MODEL` | `moonshine/base` | Moonshine model name |

## MCP servers file

`AR_MCP_SERVERS_FILE` points to a JSON file. It is either an array of servers or an object with a `servers` array. Each server has:

- `name`: `^[a-z0-9][a-z0-9_-]{0,39}$`, unique in the file.
- `url`: a streamable HTTP MCP endpoint over `https`, or plain `http` to a loopback host (`127.0.0.1`, `localhost`, `[::1]`) for a server on the control VM itself. An `https` host must resolve only to public addresses: a private, loopback, link-local or reserved address is refused when the file loads (for a literal address) or at every request (for a name). Each request connects to the address it checked, ignores proxy settings and does not follow redirects, as webhooks do.
- `headers` (optional): a map of header names to values. Values are credentials. They stay on the control plane and never appear in prompts, the box or error messages.
- `allowed_tools` (optional): when set, only these tool names are listed and callable.

At most 32 servers. Responses are capped at 4 MiB. Keep the file readable only by the service user.

```json
{"servers": [
  {"name": "docs", "url": "https://mcp.example.com/mcp",
   "headers": {"Authorization": "Bearer <token>"},
   "allowed_tools": ["search"]}
]}
```

`GET /v1/agent-config` reports the server names the operator allows, as `mcp_servers_available`. The agent's `mcp_servers` setting can only name those.

## Deployment decisions (2026-11-05)

Historical record. The selections below were made on 2026-11-05. Later checks changed some of them: the model is now verified as `glm-5.3-flash`, and its tariffs came from Vultr's catalog (see [VERIFICATION.md](VERIFICATION.md)). The tables above give the current values.

The user selected **GLM 5.3 Flash** and delegated remaining configuration and credential choices. Machine-readable decisions are in deploy/decisions.json. These are configuration selections, not a claim that cloud deployment or inference has been activated.

`AR_OPERATOR_PASSWORD` is already chosen privately; the user should not type or publish it. `AR_INFERENCE_MODEL` is a setup-managed binding for Vultr's exact catalog identifier of GLM 5.3 Flash. `AR_INPUT_USD_PER_MILLION` and `AR_OUTPUT_USD_PER_MILLION` are internal billing metadata, not prices chosen by the user and not changes to what Vultr charges. Trusted setup must obtain the provider's actual rates for the selected model and fill them only after verification. The configurable allowance is $5; it is distinct from those provider tariffs. Blank/missing tariff fields are accepted as unconfigured and keep paid inference disabled rather than assuming a zero price. No manual price selection is required from the user.

Numbers entered into those fields alone do not enable paid calls. An internal `AR_PRICING_VERIFIED` flag defaults false; trusted setup sets it true only after verifying the provider/model tariff and its units. The user need not supply this flag or choose tariff amounts.

| Setting | Choice |
| --- | --- |
| Agent model | GLM 5.3 Flash, exclusively through Vultr Serverless Inference |
| Exact API model identifier/pricing | Pending official catalog verification; no substitute model or guessed prices |
| Inference budget | $5, with existing durable reservations and per-call output bounds |
| Temperature/output | 0.2; at most 4096 output tokens |
| Main/subordinate calls | Same selected model and fixed Vultr gateway; no alternate provider |
| Preferred region | Chicago (`ord`), subject to actual availability/pricing |
| VM operating system | Ubuntu 24.04 LTS, x86-64, subject to provider availability |
| VM selection | Lowest-priced adequate control 2 vCPU/4 GiB and worker 4 vCPU/8 GiB plans |
| Overall spending | $300 ceiling, at least $50 reserve, combined target at most $0.30/hour, temporary checks at most two hours |
| Idle resources | Destroy task containers; remove newly created temporary VMs after retaining evidence/configuration |
| Default execution | Rust CLI, gVisor, no network, 512 MiB, 1 vCPU ceiling, 30-second deadline |
| Optional capabilities | Nested Docker/browser/desktop and automatic scale-out remain disabled |
| Operator authentication | Password-only UI login; internal owner `operator`; generated 288-bit random password |
| Node authentication | Separate generated 256-bit bearer token plus private mTLS |
| SSH | Dedicated Ed25519 deployment key, key-only server access, private key restricted to trusted runner |
| Private TLS | P-256 development CA valid 30 days; seven-day worker/client certificates |
| Private worker identity | `worker.agent-rush.internal`; route to the actual private VM IP only after provisioning |
| Public hostname | Derive `agent-rush.<control-public-ip>.sslip.io` after allocation; verify DNS and publicly trusted TLS before use; no domain purchase |
| Database | SQLite file; no database password/account to manage |

The exact operator password, worker token, environment files, SSH key and private TLS keys are documented in `/workspace/.agent-rush-secrets/CREDENTIALS.md`. That file and the credential directory are owner-only, outside Git and submission artifacts. Current secrets are not populated into environment-draft suggested values. Store/export them privately before any shared filesystem snapshot; do not copy the credential directory into build images or source archives.

Local configuration is `/workspace/.agent-rush-secrets/local.env`; future control and worker configurations are alongside it. The local environment disconnects the worker, uses loopback-only HTTP and keeps paid inference disabled. Production configuration keeps secure cookies enabled and uses the private mTLS worker route. Certificate validation passed at creation; certificates are not a substitute for a publicly trusted web certificate, and expiration must be checked before deployment.

The catalog lookup for the selected model currently fails at proxy CONNECT with 403. No inference key, verified API model identifier or prices are configured. `AR_INFERENCE_MODEL` is intentionally left unset until Vultr's exact identifier is confirmed; a display name or guessed slug must not be sent as a verified model ID. Keep missing prices unset rather than assigning zero. Verify the selected model exists and is available through Vultr; do not silently choose another provider/model if it does not.

Official Vultr SDK inspection confirmed that the authorized infrastructure account API can retrieve keys for existing serverless-inference subscriptions. `vultr-inference-key` exports an unambiguous existing key to an owner-only file without printing it or creating a subscription. `vultr-inference-model` uses that inference credential to query Vultr's model catalog and export only the exact selected model ID. These steps cannot run until the saved API network rules are applied. Model lookup alone does not verify billing rates or enable paid calls; setup still has to inspect the provider tariff and its units. The password and pricing choices do not require another permission question.

These choices create no VMs, API subscriptions or billable model requests. The host still fails resource-controller qualification and must refuse task execution.
