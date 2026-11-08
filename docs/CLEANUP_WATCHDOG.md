# Independent cleanup watchdog

The native Rust `vultr-watch` command is a management-runner component. It does not run in a task, control container or execution worker. Its account credential stays in an owner-only trusted-runner file. It watches exact acknowledged IDs from the existing private deployment manifest; runtime agents have no authority to create or delete resources.

Preview requires neither a cloud key nor working application settings:

```sh
agent-rush vultr-watch --manifest /private/deployment.json
```

The builder starts continuous cleanup on a management runner with independent uptime and network access for the whole test window:

```sh
agent-rush vultr-watch --execute \
  --manifest /private/deployment.json \
  --heartbeat /private/watchdog.json \
  --credential-file /private/vultr-api-key \
  --poll-seconds 15
```

Before expiry it authenticates the bound account and credential scope and publishes a private heartbeat without deleting resources. At expiry it removes tracked project VMs first, then dependencies, verifying absence and retaining partial progress. Pending deletions and API errors remain incomplete and are retried. Manual teardown can finish early; the watcher rechecks recorded cleanup and stops after verified absence. An empty initial manifest stays armed until expiry so a new watcher does not quit before the first resource is recorded.

`--once` performs one authenticated check (and cleanup if already due); its report cannot count as an armed continuous watcher. Poll intervals are bounded to 5–60 seconds, with 15 seconds as the default. Readiness requires the original manifest identity and expiry, an authenticated heartbeat no older than 30 seconds, continuous mode, and a matching live Linux PID and start timestamp. Zombie, stopped or reused PIDs fail readiness. A one-shot or old JSON file cannot establish a running watchdog.

Only one watcher can own a manifest, even with different heartbeat filenames. Its dedicated lock is separate from short manifest update locks; tracking can continue between iterations. Heartbeats use owner-only atomic replacement and cannot replace another deployment's report. The original scope, region, price estimate and start/expiry times are immutable during normal manifest saves; tracking cannot silently extend a test window.

Cleanup durably closes resource admission under the manifest lock before any API operation. Closed deployments reject new IDs, dropped or replaced IDs and stale attempts to reopen them. The fence survives failed API calls and restarts, including an early stop before the original expiry. Completion and successful new tracking cannot both occur for the same closed deployment. Future creation admission must honor this fence and persist its intent before contacting Vultr; the watchdog does not solve an in-flight creation that was never recorded.

SIGINT/SIGTERM requests an early cleanup attempt before exit. Unresolved cleanup exits nonzero and preserves private state. Do not stop the management runner while cloud resources remain. The systemd template `deploy/cleanup-watchdog.service` uses a separate deployment identity and restarts unexpected failures. It is prepared but not installed or running in the current environment. The builder must install and verify it on an appropriate management runner before relying on it.

This is not provider-side TTL, a guaranteed account-wide billing cap, or evidence of real Vultr deletion. Runner or network outages can prevent cleanup, and stopped VMs can continue billing. No live watchdog or cloud VM was started here. The API route still returns proxy CONNECT 403.

Automated VM creation remains disabled. This watchdog covers recorded acknowledged IDs; safe provisioning still needs durable create-intent recovery for ambiguous or late provider responses, current prices and credit, deployment access and live cleanup acceptance. It must never be described as discovering every account resource or deleting unrelated resources by a prefix scan. The $300 ceiling, $50 reserve and two-hour test window remain binding.
