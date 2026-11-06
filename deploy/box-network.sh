#!/usr/bin/env bash
# Create the session box network "ar-box", its egress filter and the host input guard, then
# write the marker the worker checks before it starts any box. Idempotent: rerunning
# changes nothing when the network and rules are already in place. Run as root.
#
# The root unit agent-rush-box-network.service runs this at every boot, before the worker
# (iptables rules do not survive a reboot), and again whenever Docker restarts; the box
# requalification timer reruns it too, so the host's own addresses are re-read at least
# every 45 minutes. The marker lives in /run (gone at reboot) and carries the boot id.
#
# Every rule carries the comment "$TAG" and matches the box subnet, so this script never
# touches any other rule.
#
# DOCKER-USER (forwarded traffic): boxes may reach the public internet; private, metadata,
# CGNAT, loopback, IETF-protocol, benchmarking, multicast and reserved ranges, the host's
# own addresses and SMTP ports are refused, and new connections are rate-limited per box.
#
# INPUT (the host input guard, always installed): box traffic to any of the host's own
# addresses is delivered locally, never forwarded, so DOCKER-USER cannot see it. One rule
# at the top of INPUT refuses all of it, whatever the host firewall allows, so no host
# service (SSH, the worker, Docker's API, anything listening later) is reachable from a box.
# `qualify-box` requires every probed host port to be refused by this rule.
#
#   box-network.sh [--guard-host-input]   (the flag is accepted for compatibility; the guard
#                                          is always installed)
#
# Overrides, for testing on a scratch network: AR_BOX_NET, AR_BOX_SUBNET, AR_BOX_GATEWAY,
# AR_BOX_BRIDGE, AR_BOX_TAG, AR_BOX_MARKER.
set -euo pipefail

case "${1:-}" in
  "" | --guard-host-input) ;;
  *) echo "usage: box-network.sh [--guard-host-input]" >&2; exit 2 ;;
esac

NET="${AR_BOX_NET:-ar-box}"
SUBNET="${AR_BOX_SUBNET:-172.30.0.0/24}"
GATEWAY="${AR_BOX_GATEWAY:-172.30.0.1}"
BRIDGE="${AR_BOX_BRIDGE:-ar-box0}"
TAG="${AR_BOX_TAG:-ar-box-egress}"
MARKER="${AR_BOX_MARKER:-/run/agent-rush/box-network.env}"
CHAIN=DOCKER-USER

# No marker unless every step below succeeds: a failed run withdraws it.
withdraw_on_failure() {
  local rc=$?
  [ "$rc" -eq 0 ] || rm -f "$MARKER"
}
trap withdraw_on_failure EXIT

if docker network inspect "$NET" >/dev/null 2>&1; then
  current="$(docker network inspect "$NET" --format '{{range .IPAM.Config}}{{.Subnet}}{{end}} {{index .Options "com.docker.network.bridge.enable_icc"}} {{.EnableIPv6}}')"
  if [ "$current" != "$SUBNET false false" ]; then
    echo "network $NET exists with unexpected settings ($current); remove it and rerun" >&2
    exit 1
  fi
  echo "network $NET present"
else
  docker network create --driver bridge \
    --subnet "$SUBNET" --gateway "$GATEWAY" --ipv6=false \
    -o com.docker.network.bridge.name="$BRIDGE" \
    -o com.docker.network.bridge.enable_icc=false \
    --label agent-rush.managed=true \
    "$NET" >/dev/null
  echo "network $NET created"
fi

# Destinations a box must never reach.
blocked=(
  0.0.0.0/8 10.0.0.0/8 100.64.0.0/10 127.0.0.0/8 169.254.0.0/16
  172.16.0.0/12 192.0.0.0/24 192.168.0.0/16 198.18.0.0/15 224.0.0.0/4 240.0.0.0/4
)
# The host's own addresses outside those ranges (its public IP), read afresh on every run.
while read -r addr; do
  blocked+=("${addr%/*}/32")
done < <(ip -4 -o addr show scope global | awk '{print $4}')

rules=()
for dest in "${blocked[@]}"; do
  rules+=("-s $SUBNET -d $dest -m comment --comment $TAG -j REJECT --reject-with icmp-admin-prohibited")
done
rules+=("-s $SUBNET -p tcp -m multiport --dports 25,465,587 -m comment --comment $TAG -j REJECT --reject-with tcp-reset")
rules+=("-s $SUBNET -m conntrack --ctstate NEW -m hashlimit --hashlimit-above 50/sec --hashlimit-burst 100 --hashlimit-mode srcip --hashlimit-name $NET -m comment --comment $TAG -j DROP")
rules+=("-s $SUBNET -m comment --comment $TAG -j RETURN")

# The host input guard, ahead of every other INPUT rule (only other box networks' guards
# may precede it), so no host firewall rule can accept box traffic.
guard="-s $SUBNET -m comment --comment $TAG -j REJECT --reject-with icmp-admin-prohibited"
guard_leads() {
  iptables -S INPUT | grep '^-A INPUT ' | awk -v g="-A INPUT $guard" '
    $0 == g { found = 1; exit }
    !($0 ~ /--comment ar-box[^ ]* -j REJECT --reject-with icmp-admin-prohibited$/) { exit }
    END { exit !found }'
}
if guard_leads; then
  echo "host input guard present"
else
  # shellcheck disable=SC2086
  while iptables -C INPUT $guard 2>/dev/null; do iptables -D INPUT $guard; done
  # shellcheck disable=SC2086
  iptables -I INPUT 1 $guard
  echo "host input guard installed"
fi

existing="$(iptables -S "$CHAIN" | grep -c -- "--comment $TAG\b" || true)"
in_place=true
[ "$existing" = "${#rules[@]}" ] || in_place=false
if $in_place; then
  for rule in "${rules[@]}"; do
    # shellcheck disable=SC2086
    iptables -C "$CHAIN" $rule 2>/dev/null || { in_place=false; break; }
  done
fi
if $in_place; then
  echo "egress rules present (${#rules[@]})"
else
  # Remove only this script's own rules, then insert the current set at the top, in order.
  { iptables -S "$CHAIN" | grep -- "--comment $TAG\b" || true; } | sed 's/^-A /-D /' | while read -r line; do
    # shellcheck disable=SC2086
    iptables $line
  done
  position=1
  for rule in "${rules[@]}"; do
    # shellcheck disable=SC2086
    iptables -I "$CHAIN" "$position" $rule
    position=$((position + 1))
  done
  echo "egress rules installed (${#rules[@]})"
fi

# Verify what is now in force before vouching for it.
guard_leads || { echo "host input guard is not ahead of the other INPUT rules" >&2; exit 1; }
for rule in "${rules[@]}"; do
  # shellcheck disable=SC2086
  iptables -C "$CHAIN" $rule 2>/dev/null || { echo "egress rule missing after install: $rule" >&2; exit 1; }
done

install -d -m 0755 "$(dirname "$MARKER")"
tmp="$(mktemp "$MARKER.XXXXXX")"
{
  echo "boot_id=$(cat /proc/sys/kernel/random/boot_id)"
  echo "network=$NET"
  echo "subnet=$SUBNET"
  echo "guard_host_input=1"
  echo "egress_rules=${#rules[@]}"
  echo "installed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$tmp"
chmod 0644 "$tmp"
mv -f "$tmp" "$MARKER"
echo "marker written: $MARKER"
