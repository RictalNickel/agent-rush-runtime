#!/usr/bin/env bash
# Sample worker host and sandbox-container resource use once per second as JSON lines.
# Usage (on the worker, as root): worker_sampler.sh SECONDS OUT_FILE
set -euo pipefail
secs="${1:?seconds}" out="${2:?output file}"
: > "$out"
for ((i = 0; i < secs; i++)); do
  now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  load=$(cut -d' ' -f1 /proc/loadavg)
  avail=$(awk '/MemAvailable/{print $2}' /proc/meminfo)
  # Only this project's sandboxes (the worker labels them); per-container cgroup readings.
  containers=$(docker ps -q --filter label=agent-rush.managed=true 2>/dev/null | while read -r id; do
    full=$(docker inspect -f '{{.Id}}' "$id")
    cg=/sys/fs/cgroup/system.slice/docker-$full.scope
    [ -d "$cg" ] || continue
    printf '{"id":"%s","memory_current":%s,"memory_max":"%s","cpu_max":"%s","cpu_usage_usec":%s,"pids":%s},' \
      "${id:0:12}" "$(cat "$cg/memory.current")" "$(cat "$cg/memory.max")" "$(cat "$cg/cpu.max")" \
      "$(awk '/usage_usec/{print $2}' "$cg/cpu.stat")" "$(cat "$cg/pids.current")"
  done)
  printf '{"at":"%s","load1":%s,"mem_available_kb":%s,"sandboxes":[%s]}\n' "$now" "$load" "$avail" "${containers%,}" >> "$out"
  sleep 1
done
