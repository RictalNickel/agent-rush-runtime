# Security audit (2026-11-06)

An adversarial review of the control plane, the worker's box API and the web UI at
`7c18f6c`, on branch `security-audit-2026-11-06`. This part is a source review: every
result below comes from reading the code and the deploy scripts, plus the regression
tests added with the fixes and a `cargo audit` run on the worker VM. The live probes of
the deployed instance (box containment, external surface, resource caps under load) are
recorded separately in the live-probe section.

Files here:

- `cargo-audit.txt`: the `cargo audit` run (no advisories).

## Results

| # | Area | Result | Basis |
| --- | --- | --- | --- |
| 1a | Every `/v1`, `/api` and `/mcp` route needs a credential | Pass | The `Owner` and `Operator` extractors (`src/api_v1.rs`) run before any body is read; `/api` handlers call `AppState::owner`; `/mcp` checks the bearer first (`src/mcp_server.rs`). Only `/`, the assets, `/healthz`, `/api/status`, `/api/login` and `/api/session` (which itself needs the cookie) are public |
| 1b | Bad or revoked key | Pass | `Ledger::api_key_owner` rejects anything not shaped `ar_` + 48 hex and matches only rows with `revoked_at IS NULL`; a bearer that fails is 401, never a fall-through to the cookie |
| 1c | Cookie mutations without CSRF | Pass | `AppState::owner` requires `X-CSRF-Token` for every non-GET cookie request, compared in constant time (`auth::same`) |
| 1d | One owner reaching another's sessions, files, skills, webhooks | Pass | Every store query is filtered by owner (`WHERE owner=?`); a foreign session id is 404, the same as a missing one. Keys and webhooks need the operator cookie: a bearer key gets 403, so a leaked key cannot mint keys or redirect webhooks |
| 1e | Login brute force | **Fixed (F1, `bb22792`)** | The limit was global: it stopped brute force but let any client lock the operator out |
| 1f | Cookie flags | Pass | `ar_session`: 256-bit random, stored hashed, `HttpOnly; SameSite=Strict; Secure` (unless `AR_SECURE_COOKIES=false`), one hour |
| 1g | Security headers and CSP | Pass | `default-src 'self'; script-src 'self'` (no inline or external script), `frame-ancestors 'none'`, `base-uri 'none'`, `nosniff`, `X-Frame-Options: DENY`, `no-referrer`, `no-store`; HSTS comes from Caddy (`deploy/install-control.sh`) |
| 2a | Files API: `..`, absolute, encoded and NUL paths | Pass | `boxes::resolve_path` collapses `.` and `..` lexically and keeps only `/workspace` and `/tmp`; NUL is refused at the API and again on the worker. Query strings are percent-decoded once by axum, so an encoded `..` reaches the same check |
| 2b | Symlinks out of the workspace | Pass | Each file script resolves the path again inside the box with `realpath -m` and refuses anything outside `/workspace` and `/tmp` (`RESOLVE` in `src/boxes.rs`), so a link to `/etc`, `/proc` or elsewhere is refused. The box itself cannot see the loop mount or other boxes |
| 2c | Oversized bodies | Pass | 25 MiB on the files route, 2 MiB on exec (1 MiB stdin), 512 KiB on other `/v1` JSON, 32 KiB on `/api` |
| 3 | Box containment (configuration) | Pass by configuration | `docker_create_args`: runsc, read-only root, `--cap-drop=ALL`, `no-new-privileges`, `--pids-limit=512`, uid 1000, memory equal to memory+swap, CPU cap, `nofile` 4096, `nosuid,nodev` tmpfs, no log driver. Egress filter and host input guard in `deploy/box-network.sh`. Live proof is in the live-probe section |
| 4a | Webhook SSRF | Pass | `UrlGuard`: https only, no credentials in the URL, every resolved address must be public (v4, v6, v4-mapped, 6to4, NAT64, CGNAT, link-local, reserved), the request connects to the checked address, ignores proxies, never follows redirects, and is checked again at every delivery |
| 4b | Remote MCP SSRF | **Fixed (F2, `c291234`)** | https MCP servers were only checked for the scheme |
| 5 | UI XSS (replies, tool output, file names, titles) | Pass | `web/app.js` builds every node with `createElement` and `textContent`, never `innerHTML`; markdown links become anchors only for `http:` and `https:` (`httpUrl`), so `javascript:` and `data:` stay text. Downloads of HTML or SVG are served as `text/plain` attachments with `nosniff` |
| 6 | Resource caps (code) | Pass | Per owner: 20 open sessions, 8 streams, 8 long requests, each taken before the body is read. Exec timeout 1 to 600 s, output kept to 256 KiB per stream, event pages capped. Behaviour under load is in the live-probe section |
| 8a | `unsafe` code | Pass | No `unsafe` block in `src/` |
| 8b | Dependency advisories | Pass | `cargo audit` on the worker: 228 crates, 1422 advisories in the database, none matched (`cargo-audit.txt`) |

## Findings

### F1 (low): one client could lock the operator out of login

Fixed in `bb22792`. Needs a control-plane redeploy.

`POST /api/login` kept one list of attempts for the whole process and refused every login
after ten attempts in a minute. That stopped brute force, but anyone who could reach the
site could keep the operator signed out by sending ten wrong passwords a minute.

Fix: failed logins are counted per client address, ten a minute (an IPv6 client by its
/64, so one host cannot rotate addresses), under a global ceiling of 100 a minute so that
many addresses together still cannot brute force. A slot is taken before the password is
checked, so parallel requests cannot overshoot, and a successful login gives it back. The
address is the socket peer; when the peer is loopback (Caddy on the same VM), it is the
last `X-Forwarded-For` entry, which Caddy writes from the connection it accepted. A
client connecting directly is counted by its socket address whatever it sends in that
header. The control server now records peer addresses
(`into_make_service_with_connect_info`).

Regression tests (`tests/api.rs`): one address at its limit does not lock out another,
and the limited address gets 429 even with the right password; behind the proxy only the
last forwarded entry counts; a direct client cannot dodge the limit with a forged
header; IPv6 clients are limited by /64; successful logins do not count; 100 failures
from distinct addresses trip the global ceiling.

Residual: a client with many IPv4 addresses (or /64s) can still reach the global ceiling
and block logins for a minute at a time. That is the price of the brute-force ceiling.

### F2 (low): remote MCP servers had no private-address check

Fixed in `c291234`. Needs a control-plane redeploy.

The MCP client accepted any `https` URL and followed no redirects, but it did not check
where the host resolved. An MCP server entry whose name resolved to a private address
(the worker's VPC address, cloud metadata, a loopback service) would have been called
with the configured headers. The file is operator-controlled, so this needed a mistaken
or hostile entry, or DNS for a listed name changing to a private address later.

Fix: an `https` MCP server now goes through the webhooks' `UrlGuard` at every request:
the host must resolve only to public addresses and the request connects to the address
that was checked, with a client that ignores proxy settings and does not follow
redirects (`webhooks::pinned_client`, now shared). A literal private address is refused
when the file loads. Plain `http` to a loopback host is still allowed, for a server the
operator runs on the control VM.

Regression tests (`tests/mcp_client.rs`): https URLs with private, loopback, link-local,
CGNAT, unique-local and v4-mapped literals are refused at load; `https://localhost:<port>`
loads but every call is refused as private, and the listener on that port sees no
connection.

## Not covered by the source review

- Box containment, external ports and TLS, and caps under real load: these need the live
  instance and are in the live-probe section.
- gVisor and Docker themselves, beyond the configuration they are given.

## Live probes (2026-11-06, run by the orchestrator)

Run against the public instance and both VMs from the admin machine, with throwaway sessions that were deleted afterwards (`purge=true`) and keys that were revoked. Raw outputs are in [`live/`](live/). Total model spend: $0.

| # | Probe | Result | Evidence |
| --- | --- | --- | --- |
| L1 | Inside a box: user, capabilities, secrets | Pass: uid 1000, CapEff/CapPrm/CapBnd all 0, no key, token, secret or password variable in the environment, no Docker socket | `box-probes.json` (`id_and_caps`, `env_secrets`, `secret_files`) |
| L2 | gVisor kernel | Pass: `dmesg` shows the gVisor sentry, root filesystem mounted read-only over 9p | `box-probes.json` (`gvisor`, `mounts`) |
| L3 | Cloud metadata 169.254.169.254 (curl, TCP, `fetch` helper, Chromium `browse`) | Blocked: no route to host / `ERR_ADDRESS_UNREACHABLE` | `box-probes.json`, `browse-probes.txt` |
| L4 | VPC: control 10.42.0.3 (22, 443), worker 10.42.0.4 (8090, 8443) | Blocked: no route to host | `box-probes.json` |
| L5 | Both VMs' public IPs, SSH | Blocked | `box-probes.json` |
| L6 | Box gateway ports 22, 53, 80, 443, 2375, 8090, 8443 | Blocked | `box-probes.json` (`gateway_ports`) |
| L7 | Box to box: box B served HTTP on :8000, box A connected | Blocked: no route to host | `box-probes.json` (`box_to_box`) |
| L8 | Public names that resolve to private addresses (`10.42.0.3.sslip.io`, `127.0.0.1.nip.io`) via curl, `fetch` and Chromium | Blocked (the nip.io name reaches only the box's own loopback) | `box-probes.json`, `browse-probes.txt` |
| L9 | SMTP port 25, IPv6, raw ICMP socket | Blocked / no IPv6 address / `Operation not permitted` | `box-probes.json` |
| L10 | Public web from a box (curl and Chromium) | Allowed, as designed: `https://example.com` 200 | `box-probes.json`, `browse-probes.txt` |
| L11 | Fork bomb (2000 forks) | Contained: only that box's sandbox died; the worker restarted it with its workspace kept, other boxes were untouched. The call returned a generic HTTP 502, now fixed to say the box stopped unexpectedly and restarts (commit 4c5c2e7) | `box-probes.json` (`pids_limit`), worker log |
| L12 | Write to `/etc`, `/usr/bin`; `/proc/sysrq-trigger` | Refused (read-only root, permission denied) | `box-probes.json` |
| L13 | Files API: symlinks to `/etc/passwd` and `/`, `..`, encoded `%2e%2e`, absolute paths, `/proc/self/environ` | Pass: every read outside `/workspace` and `/tmp` refused with 400 after the in-box `realpath` recheck; a write through a symlink lands only where it resolves inside `/tmp` | `box-probes.json` (`files_*`, `put_via_symlink`) |
| L14 | Every `/v1`, `/api` and `/mcp` route with no credentials and with a well-formed bad key | Pass: 401 everywhere. `/api/memories` answered 400 (query parsed before auth); now authenticates first (commit 4c5c2e7) | `auth.txt` |
| L15 | API key on operator-only routes (`POST /v1/keys`, `POST /v1/webhooks`) | Pass: 403 | `auth.txt` |
| L16 | CSRF missing or wrong on key creation; revoked key; logout | Pass: 403, 403; revoked key 401; cookie 401 after logout | `revoke-csrf.json` |
| L17 | Second API key reading the first key's session | Allowed, by design: every key the operator mints belongs to the operator, so the live instance is single-owner. Isolation between owners is enforced in code (`WHERE owner=?`) and covered by unit tests | `revoke-csrf.json`, `src/api_keys.rs` |
| L18 | Oversized JSON body (700 KB) | Pass: 413 | `tls-headers.txt` |
| L19 | Open TCP ports from the admin IP | Control: 22, 80 (redirects to https), 443. Worker: 22 only. SSH is open only to the admin IP in the Vultr firewall | `portscan.txt` |
| L20 | TLS and headers | Pass: TLS 1.2 (ECDHE-ECDSA-AES128-GCM) and 1.3 only (1.0/1.1 negotiate up to 1.3); HSTS, CSP with `script-src 'self'` and `frame-ancestors 'none'`, nosniff, DENY, no-referrer | `tls-headers.txt` |
| L21 | Load: 8 parallel sessions on the 4 vCPU / 8 GB worker | 5 started, 3 refused with a clean 503 "worker box capacity exhausted" (the 8-box ceiling plus memory). Now configurable with `AR_BOX_MAX` | `docs/BENCHMARKS.md` |
