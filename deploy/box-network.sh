#!/usr/bin/env bash
# Create the session box network "ar-box" and its egress filter. Idempotent: rerunning
# changes nothing when the network and rules are already in place.
#
# Every rule lives in DOCKER-USER, matches the box subnet and carries the comment
# "ar-box-egress", so this script never touches any other rule. Boxes may reach the
# public internet; private, metadata, CGNAT, loopback and multicast ranges, the host's
# own addresses and SMTP submission ports are refused, and new connections are
# rate-limited per box.
#
# Traffic from a box to the host's own addresses is delivered locally (INPUT), not
# forwarded, so it never passes DOCKER-USER. Without --guard-host-input the host firewall
# (ufw, default deny incoming) is what keeps the worker listener out of reach, and any port
# it opens to everyone (SSH) stays reachable from boxes. With --guard-host-input one more
# rule, also tagged and matched on the box subnet, refuses all box traffic to the host.
# `qualify-box` observes the result either way.
#
#   box-network.sh [--guard-host-input]
set -euo pipefail

GUARD_INPUT=false
[ "${1:-}" = "--guard-host-input" ] && GUARD_INPUT=true

NET=ar-box
SUBNET=172.30.0.0/24
GATEWAY=172.30.0.1
BRIDGE=ar-box0
TAG=ar-box-egress
CHAIN=DOCKER-USER

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
  172.16.0.0/12 192.168.0.0/16 224.0.0.0/4 240.0.0.0/4
)
# The host's own addresses outside those ranges (its public IP).
while read -r addr; do
  blocked+=("${addr%/*}/32")
done < <(ip -4 -o addr show scope global | awk '{print $4}')

rules=()
for dest in "${blocked[@]}"; do
  rules+=("-s $SUBNET -d $dest -m comment --comment $TAG -j REJECT --reject-with icmp-admin-prohibited")
done
rules+=("-s $SUBNET -p tcp -m multiport --dports 25,465,587 -m comment --comment $TAG -j REJECT --reject-with tcp-reset")
rules+=("-s $SUBNET -m conntrack --ctstate NEW -m hashlimit --hashlimit-above 50/sec --hashlimit-burst 100 --hashlimit-mode srcip --hashlimit-name ar-box -m comment --comment $TAG -j DROP")
rules+=("-s $SUBNET -m comment --comment $TAG -j RETURN")

if $GUARD_INPUT; then
  guard="-s $SUBNET -m comment --comment $TAG -j REJECT --reject-with icmp-admin-prohibited"
  # shellcheck disable=SC2086
  if iptables -C INPUT $guard 2>/dev/null; then
    echo "host input guard present"
  else
    # shellcheck disable=SC2086
    iptables -I INPUT 1 $guard
    echo "host input guard installed"
  fi
fi

existing="$(iptables -S "$CHAIN" | grep -c -- "--comment $TAG" || true)"
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
  exit 0
fi

# Remove only this script's own rules, then insert the current set at the top, in order.
{ iptables -S "$CHAIN" | grep -- "--comment $TAG" || true; } | sed 's/^-A /-D /' | while read -r line; do
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
