# MCP over the public URL (2026-11-06 UTC)

These are curl transcripts against `https://144-202-59-108.sslip.io/mcp` (binary `a07f24c2…`, branch `agent-backend` at `ffc7332`). Each call sent `Authorization: Bearer ar_<redacted>`. Each file holds the request, the response headers, and the body, followed by the HTTP code and curl's total time.

| File | Call | Result |
| --- | --- | --- |
| `01-initialize.txt` | `initialize` (2025-06-18) | 200: server `agent-rush` 0.1.0, tools capability, instructions |
| `02-initialized.txt` | `notifications/initialized` | 202, empty body |
| `03-tools-list.txt` | `tools/list` | 12 tools |
| `04-start-session.txt` | `start_session` `{title}` | session `d2055b5f`, idle |
| `05-send-message.txt` | `send_message` | seq 1, starts a turn |
| `06-wait-for-reply.txt` | `wait_for_reply` (300 s) | idle after 6.3 s. Reply: Python 3.12.3, Node v26.13.0, rustc 1.90.0 |
| `07-run-command-long.txt` | `run_command` `sleep 150` | exit 0 after **150.3 s** through Caddy and the worker's nginx |
| `08-send-message-slow.txt` | `send_message` (bash `sleep 100`) | seq 7 |
| `09-wait-for-reply-long.txt` | `wait_for_reply` `after_seq: 6` | held **103.9 s**, then returned the reply |

`mcp-session-events.json` is the session's full event log. Model cost was $0.0005. The session was closed afterwards.
