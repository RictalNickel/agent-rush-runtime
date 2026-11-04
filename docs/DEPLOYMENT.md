# Bounded Vultr deployment

Status: deployment preparation only. No Vultr machines, VPCs, firewall groups, SSH keys or paid inference requests have been created by this project. The current proxy denies api.vultr.com; actual account credit, plan prices, credentials and region availability are not verified.

## Prerequisites

1. Apply the saved environment network additions: api.vultr.com, api.vultrinference.com, api.github.com, www.vultr.com, lablab.ai, community.vultr.com, gvisor.dev and storage.googleapis.com. Existing package-manager presets remain. A saved draft is not an applied runtime policy or published snapshot.
2. Retrieve the distinct credential for an existing unambiguous Vultr inference subscription through the authorized account API, verify the exact GLM 5.3 Flash catalog ID and its actual input/output prices. The builder handles these settings; the user does not choose tariffs or repeat the account key. If no suitable subscription/model exists, report that fact before enabling paid use. The user explicitly authorized storing their infrastructure key outside the repository in an owner-only local file. Do not put either credential in Git, Docker build arguments, task environments, user-facing logs or slides.
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

## Trusted setup and exact-resource cleanup tooling

The Rust CLI includes the following operational commands. The builder runs them with verified provider data; these are not additional choices or pricing values required from the user.

- `vultr-inference-key --credential-file <account-key-file> --output-file <private-inference.env>`: read existing serverless subscriptions through GET /v2/inference and export the project subscription key (or the only unambiguous existing subscription). Optional `--subscription-id` identifies an inspected existing subscription. Key values are never printed, existing files are not overwritten, and no subscription or model request is created.
- `vultr-inference-model --output-file <private-model.env>`: after the inference key is securely injected as VULTR_INFERENCE_API_KEY, query the fixed Vultr model catalog and export the unique GLM 5.3 Flash ID. No other version/variant/provider is substituted. Pricing remains unverified until official tariff units/rates are inspected; this command makes no paid inference call.
- `vultr-manifest --manifest <private-path> --hourly-estimate <provider-verified-total> --credential-file <account-key-file>`: authenticate a read-only account lookup, then initialize an owner-only journal bound to a hash of that account identity and exact credential, with a fresh project nonce, validated cost/lifetime and no resources. Directory must already exist. Default region ord, two-hour lifetime. The estimate must come from the provider; initializing a manifest does not provision anything. Key rotation/account changes require explicit reconciliation, never silent rebinding.
- `vultr-track --manifest <private-path> --kind <instance|vpc|firewall|ssh-key> --id <acknowledged-new-ID> --role <control|worker|shared>`: record only newly created project resources after an actual creation acknowledgement. Use the manifest's exact tag and derived labels at creation. Instance/firewall roles are control or worker; VPC/SSH roles are shared. Never track an unrelated or reused subscription/resource.
- `vultr-teardown --manifest <private-path>`: local preview, no key/API required. Add `--execute --credential-file <account-key-file>` only for actual authorized cleanup. It authenticates and checks the bound account/credential scope before treating a lookup as absence, then checks exact live ID, project label/tag, region and creation time before deletion. Both independent VMs are attempted even when one is pending/errors; dependency cleanup waits for all VMs to be absent. VPC attachments/NAT gateways and firewall attachments must be empty. Every deletion requires a follow-up lookup confirming absence; API errors/pending deletes are not success.

Manifest updates use an owner-only sidecar lock and atomic replacement; simultaneous cleanup processes cannot race updates. Confirmed removals are saved even if a later operation fails. Expiry or exhausted spending never prevents cleanup of owned resources. Resource IDs are validated before constructing fixed Vultr API paths. Cloud cleanup does not depend on valid application/model price settings. It is fixture-tested, not a cloud acceptance result.

Source reference for endpoint/response discovery: Vultr's official govultr SDK at ab4d9052f7bba410fd63a3f9129e06bd5e14aae1. VPCs use /v2/vpcs; the existing-inference list exposes subscription keys. Key/subscription responses must never be dumped. No new VM provisioning command or independent scheduled billing cutoff is implemented by this tooling; both remain prerequisites for a live temporary deployment.

The temporary owner-only infrastructure-key file used for the blocked account check was removed before environment publication. Its value was never included in source, saved setup instructions or artifacts. The user's existing authorization remains; do not ask for the same infrastructure credential again merely because the temporary file was cleaned up. Use a secure binding or temporary trusted-runner injection when the API route works.
