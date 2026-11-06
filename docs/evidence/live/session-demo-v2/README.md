# Live session demo v2 (2026-11-06 UTC)

This is a redeploy of branch `agent-backend` at `58a20af`. It adds a boot-bound box firewall unit, a hard disk quota per workspace, parked workspaces, browser snapshots that keep direct text, and markdown replies in the web chat.

- **Binary:** sha256 `86ce69f850dc4139581726224048bbd4378239da8c9d1435f6189da02416a73b` on both VMs.
- **Workspace image:** `agent-rush-workspace@sha256:34192dbfbea41e341a9a09db1dc234f244ccd746b7fa1850193b3ba1545313aa`, rebuilt for the new `browse`.
- **Inference:** Vultr Serverless Inference (`glm-5.3-flash`).
- **Access:** all calls went through `https://144-202-59-108.sslip.io` with an API key.
- **Redaction:** keys, tokens and the worker's public IP are removed.

## Demo session `1c047b6c` (kept live)

The first turn used the same request as v1 (`demo-request.json`). One interjection, sent at seq 14, asked for a footer.

| Step | Events | Outcome |
| --- | --- | --- |
| Research | `web_search` #3, `web_fetch` MDN #5 | 418 "I'm a teapot", RFC 2324, reserved in RFC 9110 |
| Build | `write_file` ×2, `bash` http.server #7–#12 | the app served on :8000, HTTP 200 |
| Interjection #14 | `edit_file` adds `<footer>` #16 | |
| Browser test | goto, type, click, snapshot, screenshot #18–#27 | the snapshot now shows `contentinfo:` / `Built inside a gVisor box`. In v1 the footer text was dropped |
| Report | `write_file`, `convert_file` #28–#31 | `report.pdf`; reply #32 with both attachments |
| Follow-up turn #34 | `web_fetch` rfc-editor.org #36, `write_file`, `convert_file` #38–#41 | the footer is now in the report, and the RFC 2324 date is corrected to 1 April 1998 |

- The first turn took 35 s.
- 17 tool calls over both turns. Model cost **$0.0054**.
- `report.pdf` (final, sha256 `04399c2c…5838a6e63`) and `screenshot.png` (sha256 `050cf41c…6e9029b`) were downloaded through `GET /v1/sessions/{id}/files`.
- `demo-stream.sse` holds the SSE stream for both turns (43 events), captured through Caddy.
- `../../ui/live-session.png` and `live-session-mobile.png` show the session in the public web chat. Replies are now rendered as markdown.

In the first turn the model again left the footer out of the report, although the interjection asked for it. It added the footer when asked in the follow-up.

## Containment probe `2401b687` (closed afterwards)

The prompt is in `probe-request.json` and the events are in `probe-events.json`. Model cost $0.0012.

| Probe | Outcome |
| --- | --- |
| metadata `169.254.169.254` via curl, and via `web_fetch` | curl (7) could not connect; `web_fetch`: `[Errno 113] No route to host` |
| worker listener `10.42.0.4:8443` and `172.30.0.1:8090` | curl (7) could not connect |
| host SSH `172.30.0.1:22` and `10.42.0.4:22` | curl (7) could not connect |
| SMTP 587 (smtplib) and 25 | `[Errno 101] Network is unreachable`; curl (7) |
| disk: `dd` 4300 MiB into `/workspace` (3.9G filesystem) | `No space left on device` after 4,126,887,936 bytes, `dd_exit=1`; after `rm`, 3.9G is free again |
| control: `https://example.com` | 200 |

`probe-box-ssh.json` holds a direct-shell probe (`POST /v1/sessions/{id}/exec`, no model). Port 22 on the box gateway, the worker and control VPC addresses and docker0 all returned "No route to host".

## Qualification on the deployed binary

- `qualification/box-qualification.json`: `/opt/agent-rush/requalify-box.sh` passed **9/9**. That covers metadata, private, host_services (22, 53, 80, 443, 2375, 2376, 5432, 6379, 8090, 8443, 9100 and others on every host address, all refused by the guard), smtp, public_https, no_credentials, non_root_readonly, disk_quota_enforced (ENOSPC on a 512 MiB workspace) and volume_destroyed.
- `qualification/job-qualification-summary.json`: the requalify service passed **10/10**.
- `deploy/worker.txt` holds:
  - the `AR_BOX_*` settings
  - the boot marker `/run/agent-rush/box-network.env`
  - the unit states
  - the worker unit, which requires `agent-rush-box-network.service` and has groups `docker disk`
  - 18 egress rules plus the INPUT guard
  - nginx
- `deploy/control.txt` holds the binary hash and the Caddyfile.
