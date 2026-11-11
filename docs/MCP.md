# MCP server (`/mcp`)

Boxoro is also an MCP server. Any MCP client (Claude Code, Claude Desktop, Cursor and others) can start agent sessions, hand them tasks, and work in their workspaces.

- **Transport:** streamable HTTP. JSON-RPC 2.0 over `POST /mcp`, answered with `application/json`. It is stateless and has no server-sent stream, so `GET /mcp` returns 405.
- **Protocol version:** `2025-06-18`.
- **Auth:** the same API keys as the REST API (see [API.md](API.md)): `Authorization: Bearer ar_...`. A request without a valid key gets 401 with `WWW-Authenticate: Bearer`. Keys are created by the operator through the browser login (`POST /v1/keys` with the cookie and `X-CSRF-Token`); a key cannot create or revoke keys, and no MCP tool manages keys or webhooks.

## Connect

Claude Code:

```sh
claude mcp add --transport http agent https://<host>/mcp --header "Authorization: Bearer ar_..."
```

Cursor (`~/.cursor/mcp.json`, or `.cursor/mcp.json` in a project):

```json
{
  "mcpServers": {
    "agent": {
      "url": "https://<host>/mcp",
      "headers": { "Authorization": "Bearer ar_..." }
    }
  }
}
```

Other clients that take a remote HTTP server work the same way: give them the URL and the `Authorization` header.

## Tools

| Tool | Arguments | What it does |
| --- | --- | --- |
| `start_session` | `title?`, `message?`, `model?` | Creates a session with its own workspace. A `message` starts the agent working. `model` is an ID from `list_models`; the default otherwise. |
| `send_message` | `session_id`, `text` | Sends a message. If the agent is busy, it gets the message before its next step (an interjection). |
| `wait_for_reply` | `session_id`, `timeout_seconds?` (default 120, max 300), `after_seq?` | Waits until the turn ends, then returns the status, the agent's reply and a short summary of what it did. |
| `get_events` | `session_id`, `after?`, `limit?` | Reads the full event log. |
| `run_command` | `session_id`, `command`, `cwd?`, `timeout_seconds?` | Runs bash in the workspace and returns the exit code, stdout and stderr. |
| `read_file` | `session_id`, `path` | Returns text as text. Binary files come back as base64, and images also as an image block. Directories are listed. |
| `write_file` | `session_id`, `path`, `content`, `encoding?` (`utf8` or `base64`) | Creates or replaces a file. |
| `list_files` | `session_id`, `path?` | Lists a directory. |
| `cancel` | `session_id` | Stops the current turn after its current step. |
| `close_session` | `session_id`, `delete?` | Closes the session and deletes its workspace. With `delete: true` it also removes the session and its event history for good (like `DELETE /v1/sessions/{id}?purge=true`). |
| `list_sessions` | | Lists your sessions, with their models. |
| `list_models` | | Lists the models a session can run on, their prices in USD per million tokens and the default. |
| `fork_session` | `session_id`, `title?`, `model?`, `message?` | Creates a session that starts with a copy of this one's workspace files and its earlier requests and replies (not its tool steps), on `model` or the source's model. Use it to run the same task on another model. |
| `get_usage` | | Your spend, tokens and model calls in total, per model and per session, like `GET /v1/usage`. |
| `manage_skills` | `action` (`list`, `get`, `save`, `delete`), `name?`, `description?`, `body?`, `markdown?` | Manages the skills the agent can load. |

`read_file` and `write_file` move files up to 8 MiB. Use the REST files endpoint for files up to 25 MiB.

A typical exchange:

1. `start_session` with `message: "Clone github.com/example/repo, run its tests and fix the failing one"`.
2. `wait_for_reply`. If `still_running` is true, call it again.
3. `send_message` with a follow-up, then `wait_for_reply` again.
4. `read_file` or `run_command` to inspect the result, then `close_session`.

## Security audits through a session

Use `start_session` or `send_message` to ask the agent to load `security-audit`, run `security_scan doctor`, and audit a project already in its workspace. The agent's native `security_scan` tool runs inside that box; this adds no tool to Boxoro's public MCP endpoint and no REST route. Use `wait_for_reply`, `get_events` and `read_file` to collect the findings and report artifacts.

The security workspace image provides the pinned scanners. Reports distinguish completed checks from missing tools, stale databases and other incomplete coverage. See [SECURITY_STACK.md](SECURITY_STACK.md) for the workflow and profiles and [SECURITY_STACK_RESEARCH.md](SECURITY_STACK_RESEARCH.md) for scanner choices. The [optional intelligence MCP bridge](SECURITY_INTELLIGENCE.md) is a separate operator-registered control-side service for narrow identifier lookups; it is not required for local scanning.

## Errors

- **Tool failures** come back as a normal result with `isError: true` and a text message. Examples are a session that is not found, a closed session, or a workspace error. A session that belongs to another key's owner is reported as "Session not found", exactly like one that does not exist.
- **Per-owner caps** are tool failures too:
  - `start_session` or `fork_session` with 20 sessions open (not closed): `too many open sessions; close one`.
  - `start_session` or `fork_session` with a model that is not on the operator's list: `unknown model ...`, with the allowed IDs.
  - `wait_for_reply`, `run_command`, `read_file`, `write_file`, `list_files` and `fork_session` with 8 such calls already running (the REST exec, file, archive and fork calls count too): `too many requests in flight; wait for one to finish`.
- **Protocol errors** use JSON-RPC codes:

| Code | Meaning |
| --- | --- |
| -32700 | The body is not JSON |
| -32600 | Not a valid JSON-RPC request, or a batch of more than 16 messages |
| -32601 | Unknown method |
| -32602 | Unknown tool, or missing or invalid arguments |

Batches (a JSON array of requests) are supported, up to 16 messages. A longer batch gets HTTP 400 with a single `-32600` error. Notifications get no answer, and a body made only of notifications returns 202.

## Try it with curl

```sh
curl -s https://<host>/mcp -H "authorization: Bearer $AR_KEY" -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"start_session","arguments":{"message":"What Python version is installed?"}}}'
```
