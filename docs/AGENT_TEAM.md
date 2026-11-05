# Agent team

With `AR_INFERENCE_PROVIDER=anthropic` (and with `vultr`, where every role uses the one configured model), a job runs as a small team instead of one agent. Trusted code assigns roles. A model can ask for a coder or worker subordinate, but it cannot change its own role, model or tools.

| Role | Model | Effort | Prompt cache | Compaction | Tools it may use |
| --- | --- | --- | --- | --- | --- |
| Orchestrator | `claude-opus-5-5` | medium | API default (5 min) | none | plan, delegate, follow_up, memory, set_limits, response |
| Worker | `claude-haiku-5-5` | high | explicit `ttl: 5m` | at 100k input tokens | memory_save, memory_search, response |
| Coder | `claude-haiku-5-5` | xhigh | API default (5 min) | at 100k input tokens | sandbox, response |
| Reviewer | `claude-opus-5-5` | high | API default (5 min) | none | review |

Every call sets `max_tokens` to 100,000. The worker and coder use server-side threshold compaction (`compact_20260112`, beta `compact-2026-01-12`). Haiku summarizes its own conversation, and the engine then resets that context to its policy, its task and the summary turn, so the stored history stays bounded.

With `AR_INFERENCE_PROVIDER=vultr` (the live deployment), every role uses `glm-5.3-flash`. The same effort column is sent as `reasoning: {effort, max_tokens: 8192}` within a 16k output budget, so reasoning can never use more than half and every role keeps room to answer. Both parts are required. Without an effort, GLM can spend all of its output on hidden reasoning. Without the cap, the coder at xhigh did the same on a harder task (job `510a429a`: 16,384 reasoning tokens, no answer).

## Resource ceilings

The orchestrator decides how much a task may use, but not how much each run starts with:
- **`set_limits`** sets the memory ceiling (256 MiB to 1 GiB) and the time limit per run (5 to 120 s). Until it is called, runs use 512 MiB and 60 s.
- **Trusted code clamps** the ceiling to the job's admitted lease and the node's capacity.
- **Each run starts at 256 MiB and 0.5 vCPU.** The worker raises memory and CPU live as the program needs them, up to the ceiling.
- **The coder cannot change resources.** The agent that writes code has no limits tool.

In live job `37853f20`, the orchestrator chose 768 MiB and 120 s for a task that needed about 500 MiB. The run grew 256 → 512 → 768 MiB and 0.5 → 1 vCPU, and peaked at 478 MiB.

## Flow

1. The orchestrator plans and delegates. It cannot run code: a `sandbox` call from the orchestrator is rejected and the rejection is logged.
2. The coder writes a standard-library Rust program and runs it in the gVisor sandbox on the private worker, fixing and rerunning on failure. The orchestrator supervises it, can send it further instructions with `follow_up` and its `context_id`, and sees the worker's evidence when the coder returns.
3. The worker handles small, fully specified non-coding tasks such as memory lookups. It cannot execute code.
4. When the orchestrator responds after a verified run, an independent reviewer receives the task, the code, the worker's evidence and the proposed answer. A rejection sends the reviewer's notes back to the orchestrator once; the second verdict is final and is stored on the receipt.

The worker receipt (exit code, checks, cleanup) remains the only authority on whether code ran. The reviewer is a quality gate on the answer and cannot mark unexecuted work as verified.

## Prompts and limits

Each role receives only its own short system prompt (`src/agent.rs`), with no tool descriptions it cannot use. The per-role allowlist in `Role::allows` enforces this independently of the prompts.

A team job allows 20 model calls, a 15-minute lifetime and up to five persisted contexts (orchestrator, two subordinates, two reviews). The original single-agent profile (`Role::Solo`) keeps its earlier limits.

## Cost accounting

Before each call, the gateway reserves a worst case against the persisted budget: the input implied by the context size at the higher price tier, the full 100k output tokens, and a second pass when compaction can run. After the call it settles to the actual cost, computed from `usage` (summing `usage.iterations` when compaction ran, cache writes at 1.25x input, cache reads at the model's cache price) using Anthropic list prices:

| Model | Input $/M | Output $/M | Cache read $/M |
| --- | --- | --- | --- |
| claude-opus-5-5 | 4.00 | 20.00 | 0.20 |
| claude-haiku-5-5 (prompt up to 100k) | 0.10 | 0.50 | 0.01 |
| claude-haiku-5-5 (prompt over 100k) | 0.50 | 2.50 | 0.05 |

A response from a model missing from this table keeps its full reservation instead of a guessed price.

## Notes

- Haiku 5.5's 5-minute TTL is also the API default, so the worker's explicit `ttl: 5m` and the coder's default behave the same. Short prompts under the model's minimum cacheable length (512 tokens) are not cached until the conversation grows.
- Anthropic's server-side refusal fallback is not enabled, because it would substitute a model other than the ones configured above.
