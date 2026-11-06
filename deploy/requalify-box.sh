#!/usr/bin/env bash
# Reapply the box network (re-reading the host's addresses), then rerun the nine box
# containment checks and replace the box report only when every check passes. The worker
# reads the report on every box creation, so a new one takes effect at once.
# Installed as /opt/agent-rush/requalify-box.sh by deploy/install-worker.sh and
# scripts/install.sh, and run by agent-rush-requalify-box.timer.
#
# AR_BOX_PRIVATE_PROBES in worker.env: comma list of private ip:port endpoints the host can
# reach and a box must not (the control VM's SSH, say); each becomes --private-probe.
set -euo pipefail
exec 9>/run/agent-rush-requalify-box.lock
flock -n 9 || { echo "box requalification already running" >&2; exit 0; }
/opt/agent-rush/box-network.sh --guard-host-input
set -a
# shellcheck source=/dev/null
. /etc/agent-rush/worker.env
set +a
args=()
IFS=',' read -ra targets <<< "${AR_BOX_PRIVATE_PROBES:-}"
for target in "${targets[@]}"; do
  [ -n "$target" ] && args+=(--private-probe "$target")
done
report=/var/lib/agent-rush/box-qualification.json
next=/var/lib/agent-rush/box-qualification.next.json
if runuser -u agent-rush -- /opt/agent-rush/agent-rush qualify-box --output "$next" "${args[@]}"; then
  mv -f "$next" "$report"
else
  [ ! -f "$next" ] || mv -f "$next" /var/lib/agent-rush/box-qualification.failed.json
  echo "box qualification failed; boxes stay refused once the current report expires" >&2
  exit 1
fi
