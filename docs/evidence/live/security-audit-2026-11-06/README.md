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
