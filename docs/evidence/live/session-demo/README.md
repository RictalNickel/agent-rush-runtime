# Live session demo (2026-11-06 UTC)

Branch `agent-backend` at `ffc7332`, release binary sha256 `a07f24c2547b85615acb0a6547f0ec22a103896804033d68e25176ccec355c6d`, deployed to both Vultr VMs. Every model call went to Vultr Serverless Inference (`glm-5.3-flash`). Requests went through the public URL `https://144-202-59-108.sslip.io` with an API key. API keys, tokens and the worker's public IP are redacted.

## Demo session `4b80272f`

The request (`demo-request.json`) asked for four steps in one turn. One interjection was sent while the agent worked.

| Step | Tools (events) | Outcome |
| --- | --- | --- |
| Research | `web_search` (#3), `web_fetch` on MDN (#5) | 418 "I'm a teapot", RFC 2324, reserved in RFC 9110 |
| Build | `write_file` ×2, `bash` starts `python3 -m http.server` (#7–#12) | `/workspace/app/index.html` + `app.js`, HTTP 200 |
| Browser test | `browser` goto, type, click, snapshot, screenshot (#13–#22) | the page shows "418 I'm a teapot" |
| Report | `write_file` report.md, `convert_file` to PDF (#23–#27) | `report.pdf` |
| Interjection (#26) | `edit_file` adds a footer, then the browser test is repeated, report rewritten and converted (#28–#45) | the footer is visible in `screenshot.png` |
| Reply (#46) | | lists the PDF and screenshot as attachments |

- 47 events, 21 tool calls, about 58 s from the first message to the reply. Model cost **$0.0057**.
- `report.pdf` (sha256 `1eae05dd…6eb5af5a`) and `screenshot.png` (sha256 `3ad24424…d58e2c`) were downloaded through `GET /v1/sessions/{id}/files`. The download headers show `content-disposition: attachment` and `nosniff`.
- `demo-stream.sse` is the live SSE stream (`/v1/sessions/{id}/stream?after=0`) captured through the public Caddy proxy. It holds all 47 events, then 15-second keep-alives.
- `../../ui/live-session.png` and `live-session-mobile.png` show the session in the public web chat.

Shortfalls of the model in this run (not containment issues):
- The final report does not mention the footer, although the interjection asked for it.
- The report says "12 common codes" but lists 13.

## Containment probe session `3257ac93`

The agent was asked to reach cloud metadata, the worker's private listener and SMTP (`probe-request.json`, `probe-events.json`). Model cost $0.0009.

| Probe | Outcome in the event stream |
| --- | --- |
| `curl http://169.254.169.254/latest/meta-data/` | curl (7) could not connect |
| `web_fetch` on the same URL | `ok: false`, `[Errno 113] No route to host` |
| `https://10.42.0.4:8443/health` (worker mTLS ingress, VPC) | curl (7) could not connect |
| `http://172.30.0.1:8090/health` (worker listener via box gateway) | curl (7) could not connect |
| SMTP `smtp.gmail.com:587` (python smtplib) | `OSError [Errno 101] Network is unreachable` |
| SMTP `smtp.gmail.com:25` | curl (7) could not connect |
| Control: `https://example.com` | 200 |

`probe-box-ssh.json` holds a direct-shell probe (`POST /v1/sessions/{id}/exec`, no model). Box TCP connections to port 22 on the box gateway, the worker and control VPC addresses and docker0 all fail with "No route to host". The host-input guard is in place.

## Qualification on the deployed binary

- `qualification/box-qualification.json`: `agent-rush qualify-box`, **8/8 passed**. It ran with `--private-probe 10.42.0.3:22 --listener-port 8090 --listener-port 8443 --listener-port 22`, so it also covers host SSH on every host address.
- `qualification/job-qualification-summary.json`: the requalify service, **10/10 passed** (containment, timeout, output, memory, scaling, scratch, pids, cpu, cancel, concurrency).
- `deploy/worker.txt` and `deploy/control.txt` hold:
  - the box requalify script
  - the egress and host-input iptables rules
  - the worker nginx config
  - the `AR_BOX_*` settings
  - the image digest
  - the Caddyfile
  - the binary hashes

The test sessions were closed afterwards. The demo session is kept. Its workspace box is reaped after 30 idle minutes (`AR_BOX_IDLE_SECONDS=1800`), so the copies here are the durable record.
