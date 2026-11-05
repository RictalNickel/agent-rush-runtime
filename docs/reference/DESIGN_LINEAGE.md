# Design lineage: Agent Zero and Agent Rush

Agent Rush is an original Rust system. While designing its agent loop, the authors read [Agent Zero](https://github.com/agent0ai/agent-zero) (MIT) at commit `ba9b67df0f54ac78106ae85c552ef5d2458f7c9a` as a reference for how a hierarchical, tool-using agent can be structured. No Agent Zero code, prompts, UI text or tool wire formats are reused. Agent Zero's license is kept in `AGENT_ZERO_LICENSE` as a courtesy, and the project is credited in `THIRD_PARTY_NOTICES.md`.

An originality review on 2026-11-05 compared every 7-word sequence in this repository against Agent Zero's prompts, web UI, plugins and Python source. It found no shared phrases outside generic HTML and CSS boilerplate. It also found no function that is a translation of an Agent Zero function. The review led to removing two compatibility layers: a decoder for Agent Zero's `tool_name`/`tool_args` envelope, and the `memory_load` and `call_subordinate` tool names.

## Shared ideas, different designs

| Idea, as found in Agent Zero | How Agent Rush does it |
| --- | --- |
| A superior agent can hand work to subordinates (`agent.py`, `tools/call_subordinate.py`) | Typed team roles assigned by trusted code: orchestrator, coder, worker and reviewer. Depth is limited to one, with at most two subordinates. `delegate` creates a subordinate and `follow_up` continues it. Subordinates share the job's lease and call budget. |
| A loop of model turns and tool calls (`Agent.monologue`, `process_tools`) | An explicit stack of frames (`src/engine.rs`) with turn, call and lifetime budgets. Each tool is checked against the role's allowlist (`Role::allows`) before it runs, and every start and finish is a durable ledger event. |
| A response tool ends the loop (`tools/response.py`) | `response` ends the job only when the independent worker receipt shows exit 0, passing checks and verified cleanup. In team mode, a reviewer must also approve; a final rejection fails the job. |
| The model can execute code (`code_execution_tool`, a shell in a container) | No shell. The coder submits one standard-library Rust program, which runs in a fresh gVisor container on a separate worker VM with no network or credentials, non-root, a read-only root and capped memory, CPU, PIDs, time, output and scratch. The container is destroyed and its removal confirmed. |
| Persistent memory (`plugins/_memory`, FAISS embeddings) | Owner-scoped SQLite notes with literal search and quotas, treated as untrusted data and never as policy. |
| Conversation history management | A per-context byte and message budget. Haiku roles use server-side threshold compaction, and the context resets to its policy, task and summary turn. |
| Model configuration (`models.py`) | One trusted inference gateway with a per-role model, effort and cache table, and a durable worst-case cost reservation settled to provider usage. |

## Only in Agent Rush

- **Separate machines.** A control VM never runs generated code. A worker VM is reachable only over a private network with mutual TLS and a bearer token.
- **Qualification.** The worker refuses to admit work unless a report of ten real containment probes, bound to its host and image, is less than two hours old.
- **Receipts.** Every run returns its exit code, structural checks, artifact SHA-256 hashes, resource samples and a cleanup state.
- **Reviewer gate.** An independent model checks the final answer against the evidence.
- **Spend ledger.** Reservations are made before each call and settled after, with a hard budget.
- **Cloud lifecycle tooling.** Exact-resource tracking, teardown and a watchdog.

## Only in Agent Zero

A general shell and Python/Node runtimes, browser and desktop control, MCP client and server, plugins and extension hooks, skills, embedding memory, unlimited subordinate depth, parallel tools, voice, document query, many model providers and a full chat UI.

## Tests that pin these differences

- `tests/agent_runtime.rs` `only_the_native_tool_schema_is_accepted`: foreign envelopes and alias names are rejected.
- `tests/team.rs`:
  - The orchestrator cannot run code.
  - The worker role cannot execute code.
  - Review approval, rejection and outage.
  - `follow_up`.
  - Compaction.
- `tests/orchestration.rs`: the engine runs end to end with fixture model and worker transports.
