# Agent Zero reference → native Rust port

Reference: agent0ai/agent-zero at `ba9b67df0f54ac78106ae85c552ef5d2458f7c9a`. The complete downloaded source archive and checksum are recorded in AGENT_ZERO_REFERENCE.md. No GitHub fork, Python runtime, dynamic Python tool loader or upstream container is used by the product. The upstream MIT license is retained in AGENT_ZERO_LICENSE. This map describes rewritten behavior, not full upstream/plugin compatibility.

| Inspected upstream behavior | Native implementation | Verification |
| --- | --- | --- |
| agent.py AgentContext / Agent hierarchy | agent.rs logical contexts, parent identity, task binding, bounded message history | native_port.rs ownership/parent/compaction tests |
| agent.py monologue / tool dispatch | engine.rs iterative frames and typed production adapters in service.rs | orchestration.rs exercises the same production engine with fixture model/worker transports |
| helpers/tool.py before/execute/after lifecycle | Strict registry parsing, durable tool start/result events, bounded untrusted tool observations | Tool-schema rejection, retry and cancellation tests |
| tools/call_subordinate.py child creation and continuation | Two child contexts, parent-owned IDs, iterative child execution/response, continuation by context_id | Iterative subordinate and continuation/provenance tests |
| tools/response.py break-loop response | Root final explanation requires a successful independently collected worker receipt | Narration without execution and failed-cleanup tests |
| plugins/_code_execution/tools/code_execution_tool.py | Only runtime=rust; fixed authenticated private worker and gVisor envelope | Native/upstream-envelope validation; historical real containment; fresh host qualification required |
| plugins/_memory/tools/memory.py save/load/delete | memory.rs SQLite owner-scoped save/literal-search/delete, 64 notes/owner, 8 KiB/note, five search results | Durable reload, owner isolation, literal queries and quota tests |
| Context persistence / history handling | Owner/job-bound SQLite snapshots; system/task retained; deterministic history compaction | Context exports require authenticated job ownership; no additional model calls |
| models.py model routing | Fixed Vultr gateway shared by root and children; durable cost reservation and usage settlement | Provider/request/accounting tests; actual GLM ID/rates/live inference still blocked |

Supported upstream-style envelopes use `tool_name`, `tool_args` and optional bounded `thoughts`. Allowed names: response, call_subordinate, code_execution_tool, memory_save, memory_load and memory_delete. Native envelopes additionally expose plan and request_memory. Unsupported runtime names, authority fields, filters, profiles, dynamic tools, reset semantics and arbitrary extension hooks are rejected. Subordinates share the same job/resource lease and model-call budget; they are not OS isolation boundaries.

Root turns are bounded to eight, child turns to four per invocation, and all contexts share at most twelve model calls and a 300-second task lifetime. Memory retrieval is literal text search; no embedding model is claimed. Compaction removes older observations while retaining the fixed policy and original task; full tool progress remains in the bounded job event ledger. Interrupted jobs are conservatively reconciled rather than silently resumed from model narration.

Docker/gVisor, Ubuntu/Debian, Chromium used for frontend QA and third-party Rust crates remain external dependencies in their original implementations. The platform/backend/orchestration/policy code is Rust. Rewriting Docker, Linux, Chromium and cryptographic libraries is outside this port. Optional browser/nested/desktop runtime profiles are disabled until genuinely implemented and qualified; downloading an image alone does not enable them.

The native qualification command runs nine fixed real checks: containment, timeout, output, memory, scratch, PIDs, CPU, cancellation and overlapping neighbor execution. Worker startup requires a passing report for the current host fingerprint and image, no more than two hours old, and recomputes each result from its recorded receipt. This report is locally collected evidence; it is not remote attestation or an independent security audit.
