# Authority and evidence

The control API owns authentication, CSRF, task identity, idempotency, memory reservations and the event ledger. The model may choose only a validated sandbox/delegate/response tool object. Unknown fields and tool names fail closed. It never receives Docker authority or infrastructure credentials.

The private node manager owns Docker. It launches a digest-pinned runsc container with non-root identity, no network, read-only root, no host mounts, all capabilities dropped, no-new-privileges, bounded PID/CPU/memory/output/files/scratch/time. Its Docker client uses an empty config and cleared environment so Docker cannot inject controller proxy credentials into the task. A fixed system tar binary exports bounded artifact bytes; paths are inspected, never extracted on the host.

Memory admission and growth reservations are transactional. Growth is currently a ledger primitive only; no public live-expansion endpoint is exposed because a host update must be proven before claiming the extra memory is available. Failed or uncertain cleanup retains the lease. Unknown/late Docker creation must not be reported as verified cleanup. Worker and controller recovery reconcile known active task containers. The trusted deployment runner and node manager still need dedicated-host qualification against crashes and hostile concurrency.

`doctor` and every execution check actual Docker capabilities and reject missing memory/CPU/PID controllers, a missing cgroup driver, or runsc `--ignore-cgroups`. These are necessary checks, not a complete security qualification. Run the real boundary tests on the intended worker before public use.

The managed development host failed a real memory allocation test despite accepting Docker memory flags. Accordingly it cannot serve as a qualified execution worker. No weaker isolation fallback was added.

Session cookies are opaque random values, stored as hashes server-side, HttpOnly, SameSite=Strict and Secure by default. Mutations require the session's independent CSRF token. Login attempts are rate limited; evidence and artifacts are owner scoped. Generated HTML is downloaded as an attachment, never rendered on the authenticated origin. CSP disallows external scripts, framing and inline scripts. TLS and private worker ingress remain deployment responsibilities.

Known limitations: one trusted operator/workspace; no hostile multi-tenant security claim; no independent security audit; structural JSON verification is not a proof of arbitrary program semantics; Docker samples are not peak-memory attestation; cloud budget estimates do not enforce a Vultr account-level hard spending cap. Do not expose this build to untrusted public task submission before deployment qualification.
