# Agent API (`/v1`)

The `/v1` API drives agent sessions. Each session has its own persistent Linux workspace (a gVisor box on the worker). You send messages, the agent works in its workspace, and you read its replies as JSON, as a live event stream, or through a signed webhook.

The same sessions are available to MCP clients at `/mcp` (see [MCP.md](MCP.md)).

## Authentication

Send an API key on every request:

```
Authorization: Bearer ar_<48 hex characters>
```

The operator's browser login (the `ar_session` cookie) also works. With a cookie, every request that changes something also needs the `X-CSRF-Token` header from `GET /api/session` or the login response.

- Create a key with `POST /v1/keys`. The response shows the key once. Only its SHA-256 is stored.
- Revoke a key with `DELETE /v1/keys/{id}`. A revoked key gets 401 straight away.
- **Creating and revoking keys, and registering and deleting webhooks, need the operator login** (the cookie plus `X-CSRF-Token`). An API key gets 403 on `POST /v1/keys`, `DELETE /v1/keys/{id}`, `POST /v1/webhooks` and `DELETE /v1/webhooks/{id}`, so a leaked key cannot mint more keys, revoke the operator's other keys or send turn results somewhere new. `GET /v1/keys` and `GET /v1/webhooks` accept a key; they never show a key, its hash or a webhook secret.
- A missing, unknown or revoked key gets 401.
- A session that does not exist and a session that belongs to another owner both get 404 with the same body. The API never reveals that someone else's session exists.

To create a key, sign in and use the cookie:

```sh
HOST=https://agent.example.com
curl -sc jar -H 'content-type: application/json' \
  -d '{"password":"<operator password>"}' $HOST/api/login   # prints {"csrf": "..."}
curl -sb jar -H 'x-csrf-token: <csrf>' -H 'content-type: application/json' \
  -d '{"name":"laptop"}' $HOST/v1/keys                     # prints {"key": "ar_...", ...}
export AR_KEY=ar_...
```

## Errors

Errors are JSON: `{"error": "Session not found"}`.

| Status | Meaning |
| --- | --- |
| 400 | The request is invalid (the message says why) |
| 401 | No API key, or the key is unknown or revoked |
| 403 | Cookie login without the CSRF header, or an API key on a route that needs the operator login (creating or revoking keys, managing webhooks) |
| 404 | Not found, or not yours |
| 409 | The session is closed |
| 413 | The body is too large |
| 422 | The JSON does not match the expected shape |
| 429 | A per-owner cap is reached: open sessions, live streams or long requests (see Limits) |
| 502 | The workspace (worker) or the voice service failed |
| 503 | Sessions are not configured: no worker is connected (`AR_WORKER_URL`) |

## Endpoints

All times are Unix milliseconds.

| Method and path | Body | Returns |
| --- | --- | --- |
| `GET /v1/status` | | provider, worker reachability, whether sessions and voice are available, MCP server names, limits |
| `POST /v1/keys` (operator login) | `{name?}` | 201 `{id, name, prefix, created_at, key}` (the key is shown once) |
| `GET /v1/keys` | | `{keys: [{id, name, prefix, created_at, last_used_at, revoked}]}` |
| `DELETE /v1/keys/{id}` (operator login) | | `{revoked: true}` |
| `POST /v1/sessions` | `{title?, message?}` | 201 `{session, sent}`; a `message` starts the first turn |
| `GET /v1/sessions` | | `{sessions: [...]}`, newest first |
| `GET /v1/sessions/{id}` | | the session: `{id, title, status, box_id, created_at, updated_at, spend_usd, last_seq}` |
| `DELETE /v1/sessions/{id}` | | `{closed: true}`; stops any turn and deletes the workspace |
| `POST /v1/sessions/{id}/messages` | `{text}` | 202 `{interjection, seq}` |
| `POST /v1/sessions/{id}/cancel` | | `{cancelled: true}`; the turn stops after its current step |
| `GET /v1/sessions/{id}/events?after=&limit=` | | `{events, next_after, more}`; `limit` is 1-1000 (default 200) |
| `GET /v1/sessions/{id}/stream?after=` | | server-sent events (see below) |
| `GET /v1/sessions/{id}/files?path=` | | the file's bytes, or `{path, entries: [{name, is_dir, size}]}` for a directory |
| `PUT /v1/sessions/{id}/files?path=` | raw bytes (up to 25 MiB) | `{bytes, sha256}` |
| `POST /v1/sessions/{id}/exec` | `{command, cwd?, timeout_seconds?, stdin?}` | `{exit_code, stdout, stderr, timed_out, truncated}` |
| `GET /v1/skills` | | `{skills, builtin}` |
| `POST /v1/skills` | `{name, description, body}` or `{markdown}` | 201, the skill |
| `GET /v1/skills/{name}` | | the skill with its `SKILL.md` text in `markdown` |
| `PUT /v1/skills/{name}` | `{description, body}` or `{markdown}` | the skill |
| `DELETE /v1/skills/{name}` | | `{deleted: true}` |
| `GET /v1/agent-config` | | `{version, config, builtin_tools, mcp_servers_available}` |
| `PUT /v1/agent-config` | `{config, reason?}` | `{version, config}` |
| `GET /v1/agent-config/history` | | `{history: [{version, reason, author, created_at}]}` |
| `POST /v1/agent-config/revert` | `{version}` | `{version, config}`; the old version is saved as a new one |
| `GET /v1/webhooks` | | `{webhooks: [{id, url, created_at}]}` |
| `POST /v1/webhooks` (operator login) | `{url}` | 201 `{id, url, created_at, secret}` (the secret is shown once) |
| `DELETE /v1/webhooks/{id}` (operator login) | | `{deleted: true}` |
| `POST /v1/voice/transcribe` | audio bytes (up to 25 MiB) | the voice service's transcript JSON |
| `POST /v1/voice/speak` | `{text}` | audio bytes |

### Sessions and turns

A session's `status` is `idle`, `running`, `failed` or `closed`.

- A message to an idle session starts a turn. The agent works step by step (shell, files, browser, research and so on) until it replies.
- A message sent while a turn is running is an **interjection**. The agent sees it before its next step, so you can steer it mid-task.
- A turn ends with status `idle` (it replied or was cancelled) or `failed` (an error, or the step or time limit).

```sh
curl -s -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' \
  -d '{"message":"Create a Flask hello-world app in /workspace/app and test it"}' \
  $HOST/v1/sessions
# {"session": {"id": "6f1c...", "status": "running", ...}, "sent": {"interjection": false, "seq": 1}}

SID=6f1c...
curl -s -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' \
  -d '{"text":"Also add a /health route"}' $HOST/v1/sessions/$SID/messages
```

### Events

Every step is an event with a `seq` that counts up from 1 within the session:

| Kind | Payload |
| --- | --- |
| `user` | `{text}` |
| `interjection` | `{text}`, a message sent while the agent was working |
| `status` | `{status, reason?}` |
| `tool_call` | `{call}`, the tool and its arguments |
| `tool_result` | `{tool, ok, output, truncated}` |
| `assistant` | `{text, attachments}`, the agent's reply |
| `config_change` | the agent edited its own config |
| `error` | `{message}` |

```sh
curl -s -H "authorization: Bearer $AR_KEY" "$HOST/v1/sessions/$SID/events?after=0&limit=100"
```

### Live stream (SSE)

`GET /v1/sessions/{id}/stream` first replays the stored events after your cursor, then streams new ones as they happen. Each SSE message has:

- `id`: the event's `seq`
- `event`: the event kind
- `data`: the event as JSON

The cursor is the `Last-Event-ID` header (browsers' `EventSource` sends it on reconnect) or `?after=`. A keep-alive comment is sent every 15 seconds. The stream ends after a `closed` status.

```sh
curl -N -H "authorization: Bearer $AR_KEY" "$HOST/v1/sessions/$SID/stream?after=0"
```

### Files and the shell

Paths are relative to `/workspace`, or absolute under `/workspace` or `/tmp`. The worker refuses `..` escapes and symlinks that lead outside. Downloads are always sent as attachments. Anything a browser could run (HTML, SVG, scripts) is sent as `text/plain`.

```sh
curl -s -H "authorization: Bearer $AR_KEY" -T data.csv "$HOST/v1/sessions/$SID/files?path=input/data.csv"
curl -s -H "authorization: Bearer $AR_KEY" "$HOST/v1/sessions/$SID/files?path=app" | jq .
curl -s -H "authorization: Bearer $AR_KEY" -o report.pdf "$HOST/v1/sessions/$SID/files?path=report.pdf"
curl -s -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' \
  -d '{"command":"cd app && python -m pytest -q","timeout_seconds":300}' $HOST/v1/sessions/$SID/exec
```

### Agent config

The agent can edit its own config, and so can you. A config has `system_addendum`, `disabled_tools`, `custom_tools` (`{name, description, command_template}`), `mcp_servers` and `reasoning_effort` (`low`, `medium` or `high`). Every change is a new version, and any version can be restored. `mcp_servers` may only name servers the operator configured (`mcp_servers_available`).

```sh
curl -s -X PUT -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' \
  -d '{"config":{"system_addendum":"Answer in British English.","disabled_tools":["browser"]},"reason":"house style"}' \
  $HOST/v1/agent-config
```

### Webhooks

When a turn ends, each of your webhooks receives a POST:

```json
{"event":"turn.end","session_id":"6f1c...","status":"idle","reason":"replied","last_reply":"Done. The app is in /workspace/app.","seq":42,"at":1762300000000}
```

- `status` is `idle` or `failed`. `last_reply` is the agent's reply from that turn, or `null`.
- The `X-Agent-Signature` header is `sha256=` followed by the hex HMAC-SHA256 of the exact body, keyed with the webhook's `secret`.
- Each delivery has a 5 second timeout and is retried twice. Redirects are not followed, and proxy settings from the environment (`HTTPS_PROXY` and the like) are ignored, so a delivery always connects to the address that was checked.
- Webhooks belong to the operator, the owner of the sessions started through the browser login or the operator's keys.
- Register one with the operator login:

```sh
curl -sb jar -H 'x-csrf-token: <csrf>' -H 'content-type: application/json' \
  -d '{"url":"https://hooks.example.com/agent"}' $HOST/v1/webhooks   # prints {"secret": "whsec_...", ...}
```
- Webhook URLs must use `https` and resolve to public addresses. Private, loopback, link-local, carrier-grade NAT and other reserved ranges are refused. The check runs when the webhook is created and again before every delivery.

Verify a delivery (Python):

```python
import hashlib, hmac
expected = "sha256=" + hmac.new(secret.encode(), body_bytes, hashlib.sha256).hexdigest()
ok = hmac.compare_digest(expected, request.headers["X-Agent-Signature"])
```

### Voice (optional)

When the operator sets `AR_VOICE_URL`, the voice routes pass requests through to the local voice service (`/transcribe` and `/speak`). Bodies are streamed in both directions and capped in size. When voice is not configured, both routes return 404.

```sh
curl -s -H "authorization: Bearer $AR_KEY" -H 'content-type: audio/wav' --data-binary @question.wav $HOST/v1/voice/transcribe
curl -s -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' -d '{"text":"Done."}' -o reply.wav $HOST/v1/voice/speak
```

## Limits

`GET /v1/status` reports the live values.

| Limit | Default |
| --- | --- |
| Message size | 64 KiB |
| File upload or download | 25 MiB |
| Exec timeout | 120 s by default, 600 s at most |
| Exec output kept | 256 KiB per stream |
| Turn wall time | 30 min |
| Events per page | 1000 |
| API keys per owner | 50 active |
| Webhooks per owner | 10 |
| Open (not closed) sessions per owner | 20; one more is 429 `too many open sessions; close one` |
| Live event streams (SSE) per owner | 8; one more is 429 until a stream ends |
| Long requests in flight per owner (exec, file reads and writes; MCP `wait_for_reply`, `run_command`, `read_file`, `write_file`, `list_files`) | 8; one more is 429 |
| Workspace exec and file calls in flight | 4 per session and 16 in all; more wait their turn |
| MCP batch | 16 messages |
