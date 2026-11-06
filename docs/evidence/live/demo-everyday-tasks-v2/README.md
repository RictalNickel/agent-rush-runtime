# Demo: everyday tasks v2 (session `c8fcac86`, kept live)

Title: "Everyday tasks v2: data, chart, documents, files". Recorded 2026-11-06 UTC on the public instance.

- **Binary:** `42c9ee69…` (`fc15bb9`), with the repeat guard, earlier turns kept in context, exec tmpfs, and exec self-kill returning 143.
- **Workspace image:** `agent-rush-workspace@sha256:434b60c2…`, which includes numpy, pandas, matplotlib, PIL and librsvg.
- **Requalified on this build:** box checks 9/9 and job checks 10/10 (`deploy/worker.txt`).
- **Model:** Vultr `glm-5.3-flash`.
- **Prompts:** the same five messages as v1, sent as separate turns, waiting for idle between them. No hints were added.

Files: `events.json` (34 events), `stream.sse` (live SSE), and `outputs/` (downloaded through the files API). The web chat view is `../../ui/demo-everyday-tasks.png`.

| Turn | Message | Time | Outcome |
| --- | --- | --- | --- |
| 1 (#1–#8) | `sales.csv`, 36 rows; totals per region and best month | 3.9 s | **OK.** North 2,125,200, West 1,996,800, South 1,867,800. Best: 2025-12 North, 218,400 |
| 2 (#9–#14) | bar chart `revenue.png` | 4.5 s | **OK, first try.** matplotlib reads `sales.csv`, and the labelled bars match the CSV totals exactly |
| 3 (#15–#22) | memo `memo.docx` plus a PDF copy | 9.3 s | **Produced.** pdflatex was missing, so it fell back to LibreOffice. The revenue figures are right: per region, the $5,989,800 total, best month, and December company-wide at $623,400. **Units and the softest month are wrong:** see below |
| 4 (#23–#28) | `inbox`: 5 messy names to lowercase-with-dashes | 2.0 s | **OK** |
| 5 (#29–#34) | `revenue.png` to an 800 px JPG, with both sizes | 1.7 s | **OK.** PNG 40,020 B, JPG 34,193 B (800×571) |

7 tool calls, no repeat warnings, no worker errors. Cost: **$0.0034**.

## Checked against `outputs/sales.csv`

| Figure | sales.csv | Chart | Memo |
| --- | --- | --- | --- |
| Revenue North / West / South | 2,125,200 / 1,996,800 / 1,867,800 | same | same |
| Total revenue | 5,989,800 | | same |
| Best row | 2025-12 North, 218,400 | | same |
| Best month, all regions | 2025-12, 623,400 | | same |
| Units total | **49,915** (North 17,710, West 16,640, South 15,565) | | **71,910** (25,710, 24,140, 22,060): invented |
| Softest month | **2025-01, 399,000** | | **"September, $499,200"**: invented |

The memo turn ran no computation. It wrote the memo from the totals in its context, which now carry over between turns. Units and the monthly low were never computed, so GLM invented them. This is a model accuracy issue, not a product failure, but a system-prompt rule such as "compute every figure you report from the data" would help.
