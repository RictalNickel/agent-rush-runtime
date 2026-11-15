#!/usr/bin/env bash
# Build-layer helper: never called by a model or a workspace task.
set -euo pipefail
family=${1:?image family required}
optional=${2:-false}
# An explicit cache path opts a trusted image builder into reusable downloads.
# The Dockerfile supplies a BuildKit cache mount; normal/custom layers still
# delete the temporary default cache before they are committed.
cache=${BOXORO_AGENT_CACHE:-/tmp/boxoro-agent-downloads}
args=(--family "$family")
if [[ $optional == true ]]; then args+=(--include-optional); fi
packages=$(/usr/bin/python3 /opt/box/agents/install.py "${args[@]}" --list-packages)
if [[ -n $packages ]]; then
    apt-get update
    # Package coordinates come from the reviewed catalog and lock, not task input.
    read -ra package_args <<< "$packages"
    apt-get install -y --no-install-recommends "${package_args[@]}"
    rm -rf /var/lib/apt/lists/*
fi
/usr/bin/python3 /opt/box/agents/install.py "${args[@]}" --cache "$cache"
dpkg-query -W -f='${Package}\t${Version}\n' > /opt/box/agents/os-packages.tsv
if [[ -z ${BOXORO_AGENT_CACHE:-} ]]; then rm -rf /tmp/boxoro-agent-downloads; fi
