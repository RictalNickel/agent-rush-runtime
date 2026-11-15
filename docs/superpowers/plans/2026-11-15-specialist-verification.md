# Specialist verification and efficiency plan

**Goal:** Correct inappropriate tool behavior, expand useful applications of existing tools, reduce repeated work, and publish verified changes to the existing GitHub PR.

**Baseline:** Local `c402504` and remote `cba870c2751508f2253c6eeb189122b88b440a2f` share tree `d070db17843cdee806ecf1e81024cc840ec586d5`.

- [x] Inspect the current implementation, clean checkout and remote PR head.
- [x] Delegate independent tool-fit, finance, and installation reviews.
- [x] Implement scoped native/helper discovery and readiness with regression tests.
- [x] Reproduce and correct inappropriate CLI recipes and incomplete-coverage reporting.
- [x] Add verified practical workflows using existing dependencies.
- [x] Measure and improve Finance discovery/query overhead while preserving isolation.
- [x] Improve verified installation/cache/layer behavior and test failure recovery.
- [x] Integrate reviews; run native, Python, real-tool, and appropriate UI checks.
- [x] Record exact results, limits and useful invocation examples.
- [x] Commit and upload; verify the remote tree and PR head.

Implementation published as [`2852a112dd858a0ded4d404847df5ce33ea4ec59`](https://github.com/RictalNickel/agent-rush-runtime/commit/2852a112dd858a0ded4d404847df5ce33ea4ec59) on [draft PR #3](https://github.com/RictalNickel/agent-rush-runtime/pull/3). Its uploaded tree `cd6f9004889a9fae026825b20d975b2ab9709b0c` exactly matches the verified local implementation tree. The PR head was read back after the branch update. This documentation checkpoint records that completed publication; it does not change executable code.

Results and limits: [specialist review](../../SPECIALIST_REVIEW.md), [integrated test receipt](../../validation/specialist-review-tests.json), [finance measurements](../../validation/finance-efficiency-2026-11-15.json), [scope measurements](../../validation/specialist-scope-efficiency.json), and [installation measurements](../../validation/specialist-installation-review.json). No merge, image publication or deployment was performed.
