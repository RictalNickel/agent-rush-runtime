# Chosen configuration

The user selected **GLM 5.3 Flash** and delegated remaining configuration and credential choices. Machine-readable decisions are in deploy/decisions.json. These are configuration selections, not a claim that cloud deployment or inference has been activated.

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

These choices create no VMs, API subscriptions or billable model requests. The host still fails resource-controller qualification and must refuse task execution.
