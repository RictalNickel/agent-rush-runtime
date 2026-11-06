# Demo: everyday tasks (session `94434218`, kept live)

Title: "Everyday tasks: data, chart, documents, files". Recorded 2026-11-06 UTC on the public instance with binary `86ce69f8…` (`58a20af`) and Vultr `glm-5.3-flash`. Messages went in over the REST API as separate turns on one workspace, waiting for idle between them.

- `events.json`: all 158 events.
- `stream.sse`: the live SSE capture.
- `outputs/`: the files, downloaded through `GET /v1/sessions/{id}/files`.
- `../../ui/demo-everyday-tasks.png`: the session in the web chat.

| Turn | Message | Time | Outcome |
| --- | --- | --- | --- |
| 1 (#1–#8) | create `sales.csv`, 36 rows; totals per region and best month | 5.7 s | **OK.** 36 rows over 3 regions. Totals: North 2,100,750, East 1,938,000, South 1,828,320. Best: 2025-12 North, 222,600 |
| 2 (#9–#132) | bar chart `revenue.png` | 102 s | **Failed (step limit).** See bug 1 and bug 2 |
| 2b (#133–#138) | clearer follow-up: use a venv in `/workspace` | 30 s | The chart was produced (840×600), but with **invented data**: 4 regions, values in thousands, not from `sales.csv`. See bug 3 |
| 3 (#139–#146) | one-page memo `memo.docx` plus a PDF copy | 14 s | **Produced.** pandoc docx, then a PDF via a fallback engine. It summarises the invented numbers from 2b |
| 4 (#147–#152) | `/workspace/inbox`: 5 messy names, renamed to lowercase-with-dashes | 2.2 s | **OK.** For example `q1-report-final-2.txt` |
| 5 (#153–#158) | `revenue.png` to an 800 px JPG, with both sizes | 2.4 s | **OK.** PNG 21,741 B, JPG 33,111 B. The agent explained why the JPG is larger |

Cost: **$0.0177** for the session, $0.0146 of it in the failed turn 2.

## Bugs found
1. **The box mounts `/home/agent` (and `/tmp`) `noexec`, so `pip install --user` packages with C extensions cannot load.** numpy failed with "failed to map segment from shared object" (#16). A virtualenv in `/workspace` works (checked by direct exec). Fix options:
   - mount `/home/agent` with exec (keep `nosuid`), or
   - set `PYTHONUSERBASE`/`PIP_USER` to a path under `/workspace`, or
   - tell the agent in the system prompt to use `/workspace/.venv`.

   ImageMagick in the image also has no SVG delegate (`rsvg-convert` is missing, #18), so `convert file.svg` fails.
2. **No repetition guard.** After those failures, GLM sent the same command 56 times in a row (#19–#130). The command wrote a 1×1 placeholder PNG. Only the 60-step limit ended the turn. Commit `b4ffeb7` removes that limit, so a loop like this would now run until the 30-minute or spend cap. The engine should stop or redirect after N identical consecutive tool calls.
3. **Earlier turns fall out of context.** `fit_messages` (`src/session.rs`) fills the window with the newest steps first. After the 120-event turn 2, turn 1 (the CSV and its totals) no longer fit. In turn 2b the model invented regions and values without reading `sales.csv`. Older turns need a short carry-over: pin each earlier turn's request and reply, or summarise them, instead of dropping them whole.
