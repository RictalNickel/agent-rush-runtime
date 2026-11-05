# Workspace visual system

Reworked on 2026-11-05 using Anthropic's `frontend-design` skill (anthropics/skills). It replaces the earlier sidebar-and-cards workspace, which read as a generic SaaS template.

## Concept: chain of custody

The product's subject is sealed, destroyed and auditable runs, so the interface borrows from evidence handling.
- **The sealed sandbox panel** has a red tamper-tape border. Its limits update as the operator changes memory or the time limit.
- **Each run is an exhibit.** The run record has a numbered timeline, the SHA-256 seal of every file, and a "Destroyed" stamp once cleanup is verified.

The bold choices are spent in one place: the sandbox panel and the stamp. Everything else is quiet: hairline rules, no card grid, no gradients and no decorative motion.

## Tokens

| Token | Value | Use |
| --- | --- | --- |
| paper | `#f3f5f4` | page background (cool, not cream) |
| panel | `#ffffff` | inputs, sandbox panel, dialogs |
| ink | `#1b2024` | text, rules, primary buttons |
| ink-2 | `#4e5862` | secondary text |
| rule | `#cdd3d6` | hairlines |
| tamper | `#c01e2e` | the only accent: sandbox border, tape, stamp, failures |
| ok | `#1f6847` | succeeded, verified, approved |
| run | `#1f5a99` | running states, focus ring |

Type: Archivo (variable weight and width, SIL OFL 1.1), self-hosted at `/assets/archivo.woff2` because the CSP allows no third-party origins. Headlines and the stamp use condensed widths (62–75%), and body text is 100% width at 16/1.55. Monospace is used only for program output and hashes.

## Layout

- **Masthead:** wordmark; live worker signal and inference provider; safety link; operator sign in. Its content aligns with the 1240px page column.
- **Lead:** one plain-language headline and a short explanation.
- **Bench:** the task form (prompt, three example tasks, memory and time-limit segmented controls) beside the sealed sandbox panel. They stack below 960px.
- **Agent team:** four roles separated by rules, with each role's reasoning effort read from `/api/status`.
- **Runs:** a ledger table showing task, state, time, model cost and cleanup. Below 640px each row becomes a stacked entry.
- **Run record (dialog):** facts grid, the cleanup stamp, then the timeline beside the answer, program output and files.

## Copy rules

Plain words, sentence case, active verbs. No all-caps labels except the stamp, where caps are the vernacular. No middle-dot metadata strings and no arrows on buttons. The timeline names roles instead of agent ids; raw ids stay in the evidence export.

## Quality floor

- Keyboard focus is always visible, and there is a skip link to the task form.
- Reduced-motion users get no animation.
- The page has no horizontal overflow at 390px.
- Toasts are a manual popover, so they appear above open dialogs.

`qa/workspace.spec.cjs` covers the desktop flow, the mobile layout, and a real Vultr receipt (`qa/fixtures/vultr-job-7d466fa9.json`) rendered with its stamp.
