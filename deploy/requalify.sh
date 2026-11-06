#!/usr/bin/env bash
# Rerun the containment checks and replace the worker's report only when every check passes.
# The running worker adopts a new report within a minute, so in-flight runs are not interrupted.
# Installed by scripts/install.sh; the same logic is inline in deploy/install-worker.sh.
set -euo pipefail
exec 9>/run/agent-rush-requalify.lock
flock -n 9 || { echo "requalification already running" >&2; exit 0; }
set -a; . /etc/agent-rush/worker.env; set +a
out=/var/lib/agent-rush/qualification.next.json
rm -f "$out"  # qualify never overwrites an existing report
if runuser -u agent-rush -- /opt/agent-rush/agent-rush qualify --output "$out"; then
  mv -f "$out" /var/lib/agent-rush/qualification.json
else
  echo "qualification failed; worker keeps its current (expiring) report" >&2
  exit 1
fi
