# Downloaded reference

Upstream https://github.com/agent0ai/agent-zero, commit ba9b67df0f54ac78106ae85c552ef5d2458f7c9a, observed upstream HEAD during inspection. Local source checkout: /tmp/agent-rush-upstream-review. Durable source archive: /workspace/reference/agent-zero-ba9b67df0f54ac78106ae85c552ef5d2458f7c9a.tar.

Archive SHA-256: b1b02f08c416ea00aba77e63f84b80674e137747b2c8028069723cfbaa5f8ae6. The archive is outside the product Git repository; it contains the downloaded upstream source, not a second product checkout.

Inspected agent.py context/agent/tool dispatch, helpers/tool.py lifecycle, helpers/tool_policy.py and the tool-access enforcement extension, models.py ModelConfig, requirements and license. The handoff's older python/helpers paths do not match this version's layout. Native equivalents are src/agent.rs, src/service.rs, src/inference.rs, src/domain.rs and the private broker/worker modules.

The Rust controller now ports iterative root/subordinate workflows, continuation, strict tool objects and supported upstream envelopes, owner-scoped durable memory save/search/delete, context snapshots/compaction, and Vultr-only inference. See NATIVE_PORT.md for exact source mappings and tests. Browser, local shell, SSH, dynamic plugin discovery, embeddings, voice and document-processing stacks remain outside the restricted registry. No Python runtime remains in the product. Docker/gVisor, Rust/Debian and third-party Rust crates remain their original infrastructure/library implementations.
