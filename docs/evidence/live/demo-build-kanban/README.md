# Demo: build and use a kanban board (session `629d0b6f`, kept live)

Title: "Build and use: kanban board app". Recorded 2026-11-06 UTC on the public instance with binary `86ce69f8…` (`58a20af`) and Vultr `glm-5.3-flash`, over the REST API.

- `events.json`: all 197 events.
- `stream.sse`: the live SSE capture.
- `outputs/`:
  - the app source (`app.py`, `cards.json`, and `kanban-src.tgz`, made with exec then downloaded)
  - every screenshot the agent took
  - the workspace listing
- `../../ui/demo-build-kanban.png`: the session in the web chat.

| Turn | Message | Time | Outcome |
| --- | --- | --- | --- |
| 1 (#1–#123) | build the kanban app (Python `http.server` backend, `cards.json`), run it on :8000, add 3 cards, move one to Done, reload, a screenshot per step | 166 s | **OK.** Cards "Write report", "Fix bug" and "Deploy app" were added. "Write report" was moved to Done, and the reload showed all three persisted (`shot_after_reload.png`, `cards.json`). It debugged itself along the way: typing with Enter did not add cards (#20–#27), so it used a click; a selector click timed out, so it inspected the DOM with `eval` |
| Interjection #5, at 4.5 s | "Also add a delete button on each card and test deleting one" | | **Partly done.** The × button was added (#8–#11), but deleting was never tested and the reply does not mention it |
| 2 (#124–#179) | dark mode toggle, test it, final screenshot | 82 s | **OK.** The toggle switches `body.dark` and persists in `localStorage`; it was checked across reloads (`dark-mode.png`, `light-mode.png`). Small app bug: in dark mode the button still reads "🌙 Dark" |
| 3 (#180–#197) | clearer follow-up: test delete | 16 s | **OK.** "Deploy app" was deleted, and the deletion survived a reload and `cards.json` (`after-delete.png`). The reply calls the deleted card "Add card" |

The interjection was sent 4.5 s into turn 1, after the backend was written but before the server started: the trigger matched "8000" inside the written source. 92 tool calls in all. Cost: **$0.0244**.

## Bugs found
1. **A command that kills its own shell becomes worker HTTP 500.** `pkill -f 'python3 app.py'; … nohup python3 app.py &` matches and kills the `bash -lc` that runs it. The worker logs `box operation failed error=box exec failed:` with an empty message, and the tool result is `box worker returned HTTP 500` (#13, #141, #143). It should return an ordinary result, such as exit code 143 or a signal, with whatever output was captured.
2. **The web chat loses inline images to HTTP 429.** Opening this session fires about 19 image loads at once. Four got 429 from the per-session limits (`max_box_calls_per_session` 4) and show "could not be loaded" (visible in `demo-build-kanban.png`). The same files load fine one at a time. The UI should queue image loads at the limit or retry 429s.
3. **Model behaviour:**
   - the interjection was half-followed
   - the reply mislabels the deleted card
   - #128 was invalid JSON (a run of tabs). The protocol caught it, and the model recovered on the next step
