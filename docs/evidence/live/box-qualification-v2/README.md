# Box qualification, nine checks (2026-11-06)

Captured on the live worker VM against a scratch box network (`ar-box-test`,
172.31.250.0/24, its own rules tagged `ar-box-test-egress` and its own marker), so the
live `ar-box` network and the running worker were not touched. The binary was the debug
build of this branch, run as the `agent-rush` user with the `docker` and `disk` groups,
as `requalify-box.sh` runs it. The worker's public address is redacted.

- `box-qualification-scratch-passing.json`: all nine checks pass. `host_services_blocked`
  probed 23 host endpoints (every `HOST_PROBE_PORTS` port and listener on the gateway,
  SSH and the listeners on every other host address), one at a time; all 23 were
  `rejected` by the host input guard. `disk_quota_enforced` wrote 576 MiB into a 512 MiB
  workspace: dd failed with ENOSPC after 477 MiB (ext4 metadata takes the rest).
- `box-qualification-scratch-without-guard.json`: the same run with the scratch INPUT
  guard deleted. `host_services_blocked` fails: 5 host ports `open` from the box (SSH
  among them), 18 silently dropped by ufw. This is the gap the mandatory guard closes.

The eight-check reports in `../box-qualification/` predate the host guard and the disk
quota; the worker no longer accepts them.
