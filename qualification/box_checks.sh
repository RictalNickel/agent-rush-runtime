#!/usr/bin/env bash
# Box qualification probes. `agent-rush qualify-box` runs this inside a fresh box, as the
# box user, and prints one JSON object per check: {"name","passed","detail"}. It is
# compiled into the binary; nothing in a box can change it.
#
#   bash -s -- --gateway <ip> --private <ip:port,...> --listeners <ip:port,...>
#
# --gateway:   the box network's gateway, which is the host
# --private:   private-network endpoints the host itself can reach (the box must not)
# --listeners: the worker's own listener on each host address
# The eighth check, volume_destroyed, is observed on the host after the box is removed.
set -u
gateway=""
private=""
listeners=""
while [ $# -gt 0 ]; do
  case "$1" in
    --gateway) gateway="${2:-}"; shift 2 ;;
    --private) private="${2:-}"; shift 2 ;;
    --listeners) listeners="${2:-}"; shift 2 ;;
    *) shift ;;
  esac
done

# probe <host> <port>: one TCP connect, at most 6 s. Prints a JSON object; "open" means
# the connection was accepted.
probe() {
  local start end out result
  start=$(date +%s%N)
  if out=$(timeout 6 bash -c "exec 3<>/dev/tcp/$1/$2" 2>&1); then
    result=open
  else
    case "$out" in
      *"No route to host"*) result=rejected ;;
      *"refused"*) result=refused ;;
      *"unreachable"*) result=unreachable ;;
      "") result=timeout ;;
      *) result=failed ;;
    esac
  fi
  end=$(date +%s%N)
  jq -cn --arg target "$1:$2" --arg result "$result" --argjson ms $(( (end - start) / 1000000 )) \
    '{target:$target,result:$result,ms:$ms}'
}
# probe_all <ip:port,...>: a JSON array of probes, run in parallel.
probe_all() {
  local dir i=0 target
  dir=$(mktemp -d)
  IFS=',' read -ra targets <<< "$1"
  for target in "${targets[@]}"; do
    [ -n "$target" ] || continue
    probe "${target%:*}" "${target##*:}" > "$dir/$i" &
    i=$((i + 1))
  done
  wait
  cat "$dir"/* 2>/dev/null | jq -cs '.'
  rm -rf "$dir"
}
none_open() { jq -e 'length > 0 and all(.result != "open")' <<< "$1" >/dev/null && echo true || echo false; }
report() { jq -cn --arg name "$1" --argjson passed "$2" --argjson detail "$3" '{name:$name,passed:$passed,detail:$detail}'; }

# 1. Cloud metadata service.
probes=$(probe_all "169.254.169.254:80,169.254.169.254:443")
curl -s -m 5 -o /dev/null http://169.254.169.254/latest/meta-data/
curl_rc=$?
passed=$(none_open "$probes")
[ "$curl_rc" -ne 0 ] || passed=false
report metadata_blocked "$passed" "$(jq -cn --argjson probes "$probes" --argjson curl_exit "$curl_rc" '{probes:$probes,curl_exit:$curl_exit}')"

# 2. Private, CGNAT and other-bridge ranges, plus private endpoints the host can reach.
probes=$(probe_all "10.0.0.1:80,172.16.0.1:80,192.168.0.1:80,100.64.0.1:80,172.17.0.1:80${private:+,$private}")
report private_blocked "$(none_open "$probes")" "$(jq -cn --argjson probes "$probes" '{probes:$probes}')"

# 3. The worker's own listener, on every host address. SSH on the host is recorded too:
# it is open to the internet anyway, but box traffic to the host is local delivery that
# only the host firewall governs, so the report shows what that firewall allows.
probes=$(probe_all "$listeners")
ssh=$(probe "$gateway" 22)
report worker_listener_blocked "$(none_open "$probes")" "$(jq -cn --argjson probes "$probes" --argjson host_ssh "$ssh" '{probes:$probes,host_ssh_informational:$host_ssh}')"

# 4. SMTP submission and relay, on IPv4 addresses (the box has no IPv6 route, so a name
# could otherwise fail on IPv6 alone and prove nothing about the filter).
submit=$(getent ahostsv4 smtp.gmail.com | awk 'NR == 1 {print $1}')
relay=$(getent ahostsv4 gmail-smtp-in.l.google.com | awk 'NR == 1 {print $1}')
if [ -n "$submit" ] && [ -n "$relay" ]; then
  probes=$(probe_all "$submit:587,$submit:465,$relay:25")
  passed=$(none_open "$probes")
else
  probes='[]'
  passed=false
fi
report smtp_blocked "$passed" "$(jq -cn --argjson probes "$probes" --arg submit "$submit" --arg relay "$relay" '{probes:$probes,smtp_gmail_com:$submit,gmail_smtp_in:$relay}')"

# 5. Public HTTPS, through the box's own resolver.
code=$(curl -sS -o /dev/null -w '%{http_code}' -m 20 https://example.com 2>/dev/null)
resolved=$(getent ahostsv4 example.com | awk '{print $1; exit}')
passed=false
case "$code" in 2??|3??) passed=true ;; esac
report public_https_allowed "$passed" "$(jq -cn --arg url https://example.com --arg status "$code" --arg resolved "$resolved" '{url:$url,status:$status,resolved:$resolved}')"

# 6. No credentials: no secret-looking variables, no host sockets, keys or config dirs.
names=$(env | cut -d= -f1 | grep -iE 'key|token|secret|passw|credential|auth|cookie|session' | jq -Rcs 'split("\n") | map(select(length > 0))')
paths=()
for p in /var/run/docker.sock /run/docker.sock /run/secrets /etc/agent-rush /var/lib/agent-rush \
         /root/.ssh "$HOME/.ssh" "$HOME/.aws" "$HOME/.docker" "$HOME/.config/gcloud" /workspace/.ssh; do
  [ -e "$p" ] && paths+=("$p")
done
found=$(printf '%s\n' "${paths[@]}" | jq -Rcs 'split("\n") | map(select(length > 0))')
mounts=$(awk '{print $2}' /proc/mounts | jq -Rcs 'split("\n") | map(select(length > 0))')
# Value hashes let the host check that its own worker token is not in the box environment.
hashes=$(env -0 | while IFS= read -r -d '' kv; do printf '%s' "${kv#*=}" | sha256sum | cut -d' ' -f1; done | jq -Rcs 'split("\n") | map(select(length > 0))')
passed=$(jq -n --argjson n "$names" --argjson f "$found" --argjson m "$mounts" \
  '($n | length) == 0 and ($f | length) == 0 and ($m | all(test("docker.sock") | not))')
report no_credentials "$passed" "$(jq -cn --argjson names "$names" --argjson paths "$found" --argjson mounts "$mounts" --argjson env_value_sha256 "$hashes" '{secret_like_env:$names,secret_paths:$paths,mounts:$mounts,env_value_sha256:$env_value_sha256}')"

# 7. Non-root, no capabilities, no new privileges, read-only root, writable workspace.
uid=$(id -u)
nnp=$(awk '/^NoNewPrivs:/ {print $2}' /proc/self/status)
capeff=$(awk '/^CapEff:/ {print $2}' /proc/self/status)
writable=()
for p in / /usr /etc /opt/box/bin /var; do
  if touch "$p/.ar-probe" 2>/dev/null; then writable+=("$p"); rm -f "$p/.ar-probe"; fi
done
rootfs_writes=$(printf '%s\n' "${writable[@]}" | jq -Rcs 'split("\n") | map(select(length > 0))')
workspace=false
touch /workspace/.ar-probe 2>/dev/null && rm -f /workspace/.ar-probe && workspace=true
sudo=false
command -v sudo >/dev/null && sudo=true
passed=$(jq -n --argjson uid "$uid" --arg nnp "$nnp" --arg cap "$capeff" --argjson w "$rootfs_writes" --argjson ws "$workspace" --argjson sudo "$sudo" \
  '$uid != 0 and $nnp == "1" and ($cap | test("^0+$")) and ($w | length) == 0 and $ws and ($sudo | not)')
report non_root_readonly "$passed" "$(jq -cn --argjson uid "$uid" --arg no_new_privs "$nnp" --arg cap_eff "$capeff" --argjson rootfs_writable "$rootfs_writes" --argjson workspace_writable "$workspace" --argjson sudo "$sudo" '{uid:$uid,no_new_privs:$no_new_privs,cap_eff:$cap_eff,rootfs_writable:$rootfs_writable,workspace_writable:$workspace_writable,sudo_present:$sudo}')"
