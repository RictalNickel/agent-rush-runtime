# Bounded Vultr deployment

Status: deployment preparation only. No Vultr machines, VPCs, firewall groups, SSH keys or paid inference requests have been created by this project. The current proxy denies api.vultr.com; actual account credit, plan prices, credentials and region availability are not verified.

## Prerequisites

1. Apply the saved environment network additions: api.vultr.com, api.vultrinference.com, api.github.com, lablab.ai, community.vultr.com, gvisor.dev and storage.googleapis.com. Existing package-manager presets remain. A saved draft is not an applied runtime policy or published snapshot.
2. Supply the distinct Vultr inference credential, select an available model and verify its actual input/output prices. The user explicitly authorized storing their infrastructure key outside the repository in an owner-only local file. Do not put either credential in Git, Docker build arguments, task environments, user-facing logs or slides.
3. Verify GitHub create/push permission and private visibility. The selected Hackathon origin's visibility has not been established; do not push there speculatively. The requested new project is RictalNickel/agent-rush-runtime, private.
4. Ensure the trusted deployment runner can reach the selected VMs through an approved SSH or equivalent management route. This managed environment currently has no TCP destination grants. HTTP domain additions alone do not grant SSH access. Never create machines before both deployment and deletion routes work.
5. Establish independent scheduled cleanup and an authenticated way to delete only newly created project resources. Stopping a Vultr VM can continue billing. Application counters are not a provider billing hard cap.

## Inspect cost before creating machines

Build the Rust CLI, then run:

```sh
agent-rush vultr-plan --region ord --credential-file /path/to/owner-only-infrastructure-key
```

This is read-only. It queries account metadata, actual available plans and prices; chooses the smallest available plan meeting 2 vCPU/4 GiB control and 4 vCPU/8 GiB worker targets; and refuses estimates above $0.30/hour combined or a two-hour test window. A monthly/672 estimate is conservative guidance, not a final hourly quote. Verify taxes, minimum charges, existing project spend and available credit before provisioning. Preserve at least $50 of the $300 ceiling. Do not consume the remaining account balance merely because the API allows it.

No machine-creation command is implemented in the current CLI. An operator must first provide the missing network/management/cleanup prerequisites; creation automation and its real acceptance test remain outstanding. No manually edited price fixtures count as verified Vultr prices.

## Target topology

- Fresh dedicated control VM, public HTTPS gateway and loopback Rust API. Use an operator-controlled hostname or another verified TLS-capable hostname. Bind only the gateway publicly. Operator login, signed-in task execution and SameSite/CSRF requirements stay enabled.
- Fresh dedicated worker VM, same-region private VPC, host cgroup v2 with an enforcing systemd/cgroupfs driver. Docker must report MemoryLimit, CpuCfsQuota and PidsLimit true. Register runsc without `--ignore-cgroups`. Pin/verify the runsc release, Docker package and sandbox image before use; keep OS package signature and TLS/checksum verification enabled.
- Node manager binds loopback 8090. A source-restricted private mTLS ingress proxies to it. deploy/worker-nginx.conf.example is a template using client-certificate verification, not a tested deployed configuration. Do not expose the worker's raw HTTP API or Docker socket publicly.
- Control holds only its operator/session state, worker client identity/token and inference key. Worker holds its node identity/token and Docker authority. Infrastructure API/SSH credentials remain with the trusted deployment runner.

Install the release binary as /opt/agent-rush/agent-rush. Create a dedicated service identity and /var/lib/agent-rush state directory. On the worker only, the trusted manager needs Docker socket access. Copy reviewed environment files from deploy/*.env.example through the trusted runner; real files must be mode 0600. Do not execute unfilled templates. The service units are prepared but have not been exercised under systemd in this container.

## Acceptance sequence on the intended worker

1. Pull the exact digest-pinned Rust image. Run `agent-rush doctor`; failure is a blocker, never a reason to weaken isolation.
2. Run the explicit six-test sandbox suite on that worker. Require memory, timeout, output, scratch, cancellation and containment/cleanup checks to pass. Add host CPU/PID/concurrency and crash-recovery qualification before claiming production safety. Current memory-failure evidence must remain in the historical report.
3. Install mTLS, verify wrong/missing certificates and incorrect worker tokens are rejected. Verify the worker is inaccessible from the public interface.
4. Configure actual Vultr inference and verified prices. Run one authenticated product task through the native Rust controller, collect provider usage metadata and worker receipts, and verify artifact hashes and empty task inventory. Mocks and locally written fixture programs do not prove this step.
5. Verify public HTTPS, cookie flags, session expiry/CSRF, owner isolation, restart recovery, error/cancel paths, and external reachability. Capture actual UI screenshots and a containment video.
6. Delete the temporary deployment when the active check finishes. Do not leave it idle waiting for the next session. A continuously available hackathon demo requires a separately stated operating window and budget allocation.

## Resource and spending ledger

Before creating each resource, record project tag, resource type/ID, region, plan and provider-verified price, creation time, expiration no later than two hours, and exact deletion command in a mode-0600 deployment ledger outside Git. Record infrastructure and inference spending separately, then reconcile against provider billing. Never delete an unrelated VM or firewall group by label substring alone.

On partial failure, immediately delete newly created resources through the verified API route. Persist pending cleanup for retry and alert the operator if deletion cannot be confirmed. Deleting containers or powering off a VM is not evidence of VM deletion or billing cessation.

No cloud teardown was needed in this session because no resources were created. The local cleanup command is `agent-rush cleanup`, which removes only containers carrying this worker project's management label; it does not inspect or delete Vultr resources.

The temporary owner-only infrastructure-key file used for the blocked account check was removed before environment publication. Its value was never included in source, saved setup instructions or artifacts. The user's existing authorization remains; do not ask for the same infrastructure credential again merely because the temporary file was cleaned up. Use a secure binding or temporary trusted-runner injection when the API route works.
