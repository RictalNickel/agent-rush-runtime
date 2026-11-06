# Workspace visual system

Reworked on 2026-11-05 into a dark chat, following Anthropic's `frontend-design` skill, then rebuilt the same day as a client of agent sessions (`/v1`). The one-shot sealed-run view is gone from the page; the `/api/jobs` backend is unchanged.

## Concept

A conversation with an agent that works in its own sealed Linux workspace:
- A thread is one session. Your first message creates it.
- The agent's work reads as a log of compact steps: one row per tool call, with its output behind a disclosure.
- Its reply comes last, with any attached files as download links and image attachments shown inline.

## Tokens

| Token | Value | Use |
| --- | --- | --- |
| ground | `#12161b` | page background (graphite blue, not tinted black) |
| surface | `#1a2027` | composer, open steps, output, dialogs |
| raised | `#232b34` | your message bubbles, hover |
| rule | `#2c3540` | hairlines |
| text / muted / faint | `#e6eaee` / `#9aa5b1` / `#6f7a86` | text levels |
| amber | `#e8b04b` | the only accent: send button, focus ring, the open session's marker, "Sent while working" |
| ok / bad / run | `#5bbf8a` / `#ec6e76` / `#6aa8e8` | state colours only: step icons, status dots, "Working" |

Amber comes from hazard tape, the vernacular of containment, and avoids the generic "near-black with vermilion or acid green" look. The font is Archivo (SIL OFL 1.1), self-hosted at `/assets/archivo.woff2` because the CSP allows no third-party origins. Condensed widths are used for the welcome headline, the wordmark and dialog titles. Monospace is used only for tool output and paths.

## Layout

- **Bar:** wordmark, live worker signal and inference provider, a Safety link, and sign in or sign out. Below 860 px a menu button opens the drawer.
- **Drawer** (248 px): New session, then the sessions newest first, each with a status dot (green ready, pulsing blue working, red failed, hollow closed).
  - Each row has a trash button ("Delete session <title>"). It shows on hover or keyboard focus, and always on touch screens and in the narrow drawer. It asks first in a small dialog ("Delete this session?", Cancel focused, Delete in the bad red, not amber), then calls `DELETE /v1/sessions/{id}?purge=true`. Deleting the open session returns to the welcome.
  - "Clear closed sessions" under the list (only while a closed session is listed) asks the same way, then calls `POST /v1/sessions/purge-closed`.
- **Chat head:** the session title, its state, the speaker toggle (voice only) and Files.
- **Thread** (760 px column): a welcome headline and four example tasks when no session is open. Then the session's events in order:
  - your messages right-aligned; an interjection gets a dashed amber edge and the tag "Sent while working"
  - tool steps as one-line rows: an icon per tool family and a plain summary ("Ran npm test — exit 0", "Opened example.com", "Edited src/app.py"). The icon is blue while running, green when done and red on failure. Opening a row shows the output (monospace, first 8,000 characters).
  - browser screenshots (`/workspace/...png` in a browser result) inline under their step
  - notes for stops, failures, errors and config changes
- **Composer:** fixed at the bottom. Enter sends and Shift+Enter adds a line. Send stays enabled while the agent works, so a message becomes an interjection. Stop appears beside Send while a turn runs. The mic appears only when voice is available.
- **Files:** a dialog listing `/workspace` with folders to open and files as download links.

## Data flow

- Live updates come from `EventSource` on `/v1/sessions/{id}/stream` (the cookie authenticates GET). It resumes with `Last-Event-ID`. If the stream is refused, or three reconnects fail in a row, the page polls `/events?after=` every 2 s instead.
- Events are applied once each, by `seq`.
- Mutations send the `X-CSRF-Token` from `/api/session`.
- Downloads are plain same-origin links (the files API sends attachments). Screenshots and spoken replies are fetched as blobs and shown through `blob:` URLs.
- **Voice** (only when `/v1/status` reports it): the mic records with `MediaRecorder`, posts the clip to `/v1/voice/transcribe` as the multipart field `audio`, and sends the text. The speaker toggle reads each new live reply aloud through `/v1/voice/speak`. Replayed history is never spoken.

## Security headers

The CSP stays `default-src 'self'` with no inline scripts or styles; sizes are set through the CSSOM. The session chat needed three small additions:
- `img-src` adds `blob:`, for screenshots
- `media-src 'self' blob:`, for spoken replies
- `Permissions-Policy: microphone=(self)`, for the mic (camera and geolocation stay denied)

## Copy rules

Plain words, sentence case, active verbs. No all-caps labels. No middle-dot metadata strings. Step summaries say what the agent did, in the past tense once done.

## Quality floor

- Keyboard focus is always visible, and there is a skip link to the message box.
- The thread is a `role="log"` live region, so new messages and steps are announced.
- Reduced-motion users get no animation.
- The page has no horizontal overflow at 390 px, even with long commands and titles.
- Toasts are a manual popover, so they appear above dialogs.

`qa/workspace.spec.cjs` runs against the real server (page, headers, login) with a fake `/v1` backend, using route interception. It covers:
- send, tool rows, inline screenshot, interjection, Stop, a reply with attachments and the files panel, over a fake SSE stream
- voice: the mic with a fake microphone, and the speaker
- 390 px mobile, with the stream refused so the page falls back to polling
- deleting a session through the confirm dialog, clearing closed sessions, and the always-visible delete button on mobile

Screenshots are in `docs/evidence/ui/`.
