# Workspace visual system

Reworked on 2026-11-05 into a dark chat, following Anthropic's `frontend-design` skill. The operator asked for a simpler interface: no resource controls, no agent-team panel, and a chat-like dark theme.

## Concept

A conversation with a containment system:
- Your task is a message.
- The agent's reply carries its answer, a memory meter for the sandbox and a "Destroyed" stamp once cleanup is verified.
- Everything else (timeline, program output, file hashes, evidence downloads) sits behind one disclosure per run.

The one loud element is the **memory meter**. A hazard-striped track shows the ceiling the orchestrator set. A dim amber fill shows the limit the run grew to, a bright amber fill shows peak use, and a thin marker shows where the run started. The text says it plainly, for example: "Memory started at 256 MiB and grew live to 768 MiB of a 768 MiB ceiling. Peak use 478 MiB."

## Tokens

| Token | Value | Use |
| --- | --- | --- |
| ground | `#12161b` | page background (graphite blue, not tinted black) |
| surface | `#1a2027` | composer, disclosures, dialogs |
| raised | `#232b34` | your message bubbles, hover |
| rule | `#2c3540` | hairlines |
| text / muted / faint | `#e6eaee` / `#9aa5b1` / `#6f7a86` | text levels |
| amber | `#e8b04b` | the only accent: meter, stamp, send button, focus ring |
| ok / bad / run | `#5bbf8a` / `#ec6e76` / `#6aa8e8` | state colours only |

Amber comes from hazard tape, the vernacular of containment, and avoids the generic "near-black with vermilion or acid green" look. The font is Archivo (SIL OFL 1.1), self-hosted at `/assets/archivo.woff2` because the CSP allows no third-party origins. Condensed widths are used for the welcome headline, the wordmark and the stamp. Monospace is used only for program output and hashes.

## Layout

- **Bar:** wordmark, live worker signal and inference provider, a Safety link, and sign in or sign out.
- **Thread** (760 px column): a welcome headline and four example tasks when empty. Then one turn per run, oldest first.
- **Turn:** your message right-aligned, then the agent's status ("Running in the sandbox", with Stop), answer, memory meter, facts (outcome, time, sandbox time, model cost, review) and the stamp.
- **Composer:** fixed at the bottom. Enter sends and Shift+Enter adds a line. The note under it explains the growth rule.

Each message starts a fresh run. Turns are updated in place by job id on every 2.5 s poll, so an open disclosure stays open and the scroll position is kept.

## Copy rules

Plain words, sentence case, active verbs. No all-caps labels except the stamp. No middle-dot metadata strings. The timeline names roles ("The orchestrator used one of its tools") instead of agent ids; raw ids stay in the evidence export.

## Quality floor

- Keyboard focus is always visible, and there is a skip link to the message box.
- Reduced-motion users get no animation.
- The page has no horizontal overflow at 390 px.
- The meter has an ARIA description and sets its sizes through the CSSOM, because the CSP allows no inline styles.
- Toasts are a manual popover, so they appear above dialogs.

`qa/workspace.spec.cjs` covers the chat flow, the mobile layout, and a real scaled Vultr run (`qa/fixtures/scaled-job.json`, job `37853f20`) rendered with its meter and stamp.
