# Design

Agent Rush is an original Rust system. Its core rule: authority stays in trusted code. Models only choose a validated tool from their role's allowlist; they never hold credentials, Docker access or the power to exceed a limit.

## How the agent works

| Concern | Design |
| --- | --- |
| Team | Trusted code assigns typed roles: orchestrator, coder, worker and reviewer. Depth is limited to one, with at most two subordinates. `delegate` creates a subordinate and `follow_up` continues it. Subordinates share the job's lease and call budget. |
| Loop | An explicit stack of frames (`src/engine.rs`) with turn, call and lifetime budgets. Each tool is checked against the role's allowlist (`Role::allows`) before it runs, and every start and finish is a durable ledger event. |
| Finishing | `response` ends the job only when the independent worker receipt shows exit 0, passing checks and verified cleanup. In team mode a reviewer must also approve; a final rejection fails the job. |
| Code execution | No shell. The coder submits one standard-library Rust program, which runs in a fresh gVisor container on a separate worker VM: no network or credentials, non-root, read-only root, capped PIDs, time, output and scratch. The container is destroyed and its removal confirmed. |
| Resources | The orchestrator sets a ceiling with `set_limits`; trusted code clamps it. Each run starts at 256 MiB and 0.5 vCPU and the worker grows it live, up to the ceiling. |
| Memory | Owner-scoped SQLite notes with literal search and quotas, treated as untrusted data and never as policy. |
| Context | A per-context byte and message budget. Haiku roles (Anthropic provider) use server-side threshold compaction; the context then resets to its policy, task and summary turn. |
| Models | One trusted inference gateway with a per-role model, effort and cache table, and a durable worst-case cost reservation settled to provider usage. |

## Design choices

- **Separate machines.** A control VM never runs generated code. A worker VM is reachable only over a private network with mutual TLS and a bearer token.
- **Qualification.** The worker refuses to admit work unless a report of ten real containment probes, bound to its host and image, is less than two hours old.
- **Receipts.** Every run returns its exit code, structural checks, artifact SHA-256 hashes, resource samples, live-scaling events and a cleanup state.
- **Reviewer gate.** An independent model checks the final answer against the evidence.
- **Spend ledger.** Reservations are made before each call and settled after, with a hard budget.
- **Cloud lifecycle tooling.** Exact-resource tracking, teardown and a watchdog.

## Deliberately not included

A general shell, browser and desktop control, plugins, embedding memory, unlimited subordinate depth and parallel tools. Each would widen what a model can reach beyond the sandbox boundary.

## Tests that pin the design

- `tests/agent_runtime.rs`:
  - Only the native tool schema is accepted.
  - Only the orchestrator can set resource ceilings, and only within policy.
- `tests/team.rs`:
  - The orchestrator and the worker role cannot execute code.
  - Review approval, rejection and outage.
  - `follow_up`.
  - Compaction.
- `tests/orchestration.rs`: ceiling clamping, defaults, and restarting at the ceiling after a memory kill.
- `tests/qualification.rs`: the `scaling` check accepts only live growth that stops at the ceiling.
