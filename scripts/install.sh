#!/usr/bin/env bash
# Install Agent Rush from this checkout on Ubuntu 24.04.
#
# Usage: scripts/install.sh [--role all|control|worker] [--with LIST] [--yes] [--dry-run]
#   --role     all (default): control and worker on this host. This is a weaker separation than two
#              VMs: the control plane and the sandboxes share one kernel and one disk.
#              control: the public control plane. worker: the private execution node.
#   --with     comma list: browser (workspace image for boxes, worker role),
#              voice-stt and voice-tts (optional voice service, control role),
#              tls (Caddy with an sslip.io name, control role)
#   --yes      run without asking for confirmation (required without a terminal)
#   --dry-run  print the plan and what would change; change nothing
#
# Idempotent: each step checks the host, changes only what differs, and reports what it changed.
# Secrets are generated or passed in through the environment, written to 0600 files, and never printed.
#
# Environment (all optional):
#   AGENT_RUSH_BINARY       prebuilt agent-rush binary to install instead of building
#   AR_SKIP_TESTS=1         skip cargo test after a source build
#   AR_PUBLIC_IP            public IPv4 for the sslip.io name with tls (detected when unset)
#   AR_WORKER_TOKEN         role control: the worker's bearer token (copy it from the worker's env file)
#   AR_WORKER_PRIVATE_IP    role control: the worker's private address, for the internal name
#   AR_SETUP_PROVIDER, AR_SETUP_INFERENCE_KEY, AR_SETUP_ADMIN_PASSWORD
#                           written by scripts/setup.sh; never echoed
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX=/opt/agent-rush
BIN="$PREFIX/agent-rush"
DATA=/var/lib/agent-rush
ETC=/etc/agent-rush
UNITS=/etc/systemd/system
WORKER_PORT=8090
WORKER_URL_SPLIT="https://worker.agent-rush.internal:8443"
SANDBOX_IMAGE="$(sed -n 's/.*"cli_sandbox": *"\([^"]*\)".*/\1/p' "$REPO/deploy/images.lock.json")"

ROLE=all
WITH=""
ASSUME_YES=0
DRY_RUN=0
DO_CONTROL=0
DO_WORKER=0
FEAT_BROWSER=0
FEAT_VOICE_STT=0
FEAT_VOICE_TTS=0
FEAT_TLS=0
CHANGES=()
LAST_CHANGED=0
RESTART_WORKER=0
RESTART_CONTROL=0
PUBLIC_HOST=""

say() { printf '%s\n' "$*"; }
step() { printf '\n== %s\n' "$*"; }
note() { printf '   %s\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }
gen_hex() { openssl rand -hex "$1"; }

# Record a real change. Dry runs print nothing here: the "would" lines already say it.
changed() {
  [ "$DRY_RUN" -eq 1 ] && return 0
  CHANGES+=("$*")
  LAST_CHANGED=1
  note "changed: $*"
}

# Run a command, or print it in a dry run.
act() {
  if [ "$DRY_RUN" -eq 1 ]; then
    note "would run: $*"
    return 0
  fi
  "$@"
}

# install_file SRC DEST MODE: copy only when the content differs.
install_file() {
  local src="$1" dest="$2" mode="$3"
  LAST_CHANGED=0
  if [ -f "$dest" ] && cmp -s "$src" "$dest"; then
    note "unchanged: $dest"
    return 0
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    note "would write: $dest"
    return 0
  fi
  install -D -m "$mode" "$src" "$dest"
  changed "wrote $dest"
}

# Environment file helpers. Values are never printed.
# A dry run that finds no file reads the example it would be created from, so the plan is accurate.
declare -A EXAMPLE_OF=()
env_source() {
  if [ -f "$1" ]; then
    printf '%s' "$1"
  elif [ "$DRY_RUN" -eq 1 ] && [ -n "${EXAMPLE_OF[$1]:-}" ]; then
    printf '%s' "${EXAMPLE_OF[$1]}"
  else
    printf '%s' "$1"
  fi
}

env_get() {
  local src
  src="$(env_source "$1")"
  [ -f "$src" ] || return 0
  sed -n "s/^$2=//p" "$src" | tail -n 1
}

# env_set FILE KEY VALUE fill|force: fill sets the key only when empty; force always sets it.
env_set() {
  local file="$1" key="$2" value="$3" mode="$4" current tmp
  current="$(env_get "$file" "$key")"
  if [ "$mode" = fill ] && [ -n "$current" ]; then return 0; fi
  if [ "$current" = "$value" ]; then return 0; fi
  if [ "$DRY_RUN" -eq 1 ]; then
    note "would set $key in $file (value not shown)"
    return 0
  fi
  tmp="$(mktemp "$file.XXXXXX")"
  grep -v "^$key=" "$file" > "$tmp" || true
  printf '%s=%s\n' "$key" "$value" >> "$tmp"
  chmod 0600 "$tmp"
  mv -f "$tmp" "$file"
  changed "set $key in $file (value not shown)"
}

# env_file_from_example DEST EXAMPLE: create a 0600 file once. Existing files keep their values.
env_file_from_example() {
  local dest="$1" example="$2"
  EXAMPLE_OF["$dest"]="$example"
  if [ -f "$dest" ]; then
    note "exists: $dest (its values are kept)"
    return 0
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    note "would create $dest (mode 0600) from $(basename "$example")"
    return 0
  fi
  install -D -m 0600 "$example" "$dest"
  changed "created $dest (mode 0600) from $(basename "$example")"
}

has_feature() {
  case "$1" in
    browser) [ "$FEAT_BROWSER" -eq 1 ] ;;
    voice-stt) [ "$FEAT_VOICE_STT" -eq 1 ] ;;
    voice-tts) [ "$FEAT_VOICE_TTS" -eq 1 ] ;;
    tls) [ "$FEAT_TLS" -eq 1 ] ;;
    *) return 1 ;;
  esac
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --role) ROLE="${2:?--role needs all, control or worker}"; shift ;;
      --role=*) ROLE="${1#--role=}" ;;
      --with) WITH="${2:?--with needs a comma list}"; shift ;;
      --with=*) WITH="${1#--with=}" ;;
      --yes) ASSUME_YES=1 ;;
      --dry-run) DRY_RUN=1 ;;
      -h | --help) sed -n '2,22p' "$0"; exit 0 ;;
      *) die "unknown option: $1 (see --help)" ;;
    esac
    shift
  done

  case "$ROLE" in
    all) DO_CONTROL=1; DO_WORKER=1 ;;
    control) DO_CONTROL=1 ;;
    worker) DO_WORKER=1 ;;
    *) die "--role must be all, control or worker" ;;
  esac

  local item
  local -a requested=()
  IFS=',' read -r -a requested <<< "$WITH" || true
  for item in "${requested[@]}"; do
    case "$item" in
      "") ;;
      browser) FEAT_BROWSER=1 ;;
      voice-stt) FEAT_VOICE_STT=1 ;;
      voice-tts) FEAT_VOICE_TTS=1 ;;
      tls) FEAT_TLS=1 ;;
      *) die "unknown feature: $item (use browser, voice-stt, voice-tts, tls)" ;;
    esac
  done

  if [ "$FEAT_BROWSER" -eq 1 ] && [ "$DO_WORKER" -eq 0 ]; then
    note "browser is ignored for role control: boxes run on the worker"
  fi
  if { [ "$FEAT_VOICE_STT" -eq 1 ] || [ "$FEAT_VOICE_TTS" -eq 1 ] || [ "$FEAT_TLS" -eq 1 ]; } && [ "$DO_CONTROL" -eq 0 ]; then
    note "voice and tls are ignored for role worker: they serve the control plane"
  fi
}

preflight() {
  step "Preflight"
  local os_name="unknown" os_version=""
  if [ -f /etc/os-release ]; then
    os_name="$(env_get /etc/os-release NAME | tr -d '"')"
    os_version="$(env_get /etc/os-release VERSION_ID | tr -d '"')"
  fi
  note "host: ${os_name:-unknown} ${os_version:-}, $(nproc) CPU, $(awk '/MemTotal/ {printf "%.1f GiB", $2/1048576}' /proc/meminfo) RAM"
  if [ "$os_version" != "24.04" ]; then
    note "warning: tested on Ubuntu 24.04 only"
  fi
  if [ "$DO_WORKER" -eq 1 ]; then
    if [ -f /sys/fs/cgroup/cgroup.controllers ]; then
      note "cgroup v2: yes"
    else
      note "cgroup v2: missing. The worker's qualification will refuse this host."
    fi
  fi
  if [ "$DRY_RUN" -eq 0 ] && [ "$(id -u)" -ne 0 ]; then
    die "run as root (sudo scripts/install.sh ...), or add --dry-run"
  fi
  if [ -z "$SANDBOX_IMAGE" ]; then
    die "deploy/images.lock.json has no cli_sandbox entry"
  fi
}

confirm() {
  [ "$DRY_RUN" -eq 1 ] && return 0
  if [ "$ASSUME_YES" -eq 1 ]; then return 0; fi
  if [ ! -t 0 ]; then
    die "no terminal to confirm on: pass --yes to run non-interactively"
  fi
  local answer
  printf '\nThis changes system packages, /etc/agent-rush, systemd units and services.\n'
  printf 'Restarts (docker, agent-rush-worker, agent-rush-control) interrupt running work.\n'
  read -r -p 'Continue? [y/N] ' answer
  case "$answer" in y | Y | yes | YES) ;; *) die "stopped; nothing changed" ;; esac
}

packages() {
  step "System packages"
  local -a want=(ca-certificates curl gnupg jq openssl git)
  if [ -z "${AGENT_RUSH_BINARY:-}" ]; then want+=(build-essential pkg-config); fi
  if [ "$DO_WORKER" -eq 1 ]; then want+=(docker.io); fi
  if has_feature tls; then want+=(debian-keyring debian-archive-keyring apt-transport-https); fi
  local -a missing=()
  local pkg
  for pkg in "${want[@]}"; do
    dpkg -s "$pkg" >/dev/null 2>&1 || missing+=("$pkg")
  done
  if [ "${#missing[@]}" -eq 0 ]; then
    note "unchanged: all packages present"
    return 0
  fi
  note "missing: ${missing[*]}"
  act env DEBIAN_FRONTEND=noninteractive apt-get update -q
  act env DEBIAN_FRONTEND=noninteractive apt-get install -yq "${missing[@]}"
  changed "installed ${missing[*]}"
}

docker_runsc() {
  step "Docker and gVisor (runsc)"
  if have docker; then note "present: docker"; else note "docker is installed by the packages step"; fi

  if have runsc; then
    note "present: runsc"
  else
    note "runsc missing: installing from the signed gVisor apt repository"
    if [ "$DRY_RUN" -eq 1 ]; then
      note "would run: add gVisor key and repository, apt-get install runsc"
    else
      install -d -m 0755 /usr/share/keyrings
      curl -fsSL https://gvisor.dev/archive.key | gpg --dearmor --yes -o /usr/share/keyrings/gvisor-archive-keyring.gpg
      echo "deb [arch=amd64 signed-by=/usr/share/keyrings/gvisor-archive-keyring.gpg] https://storage.googleapis.com/gvisor/releases release main" \
        > /etc/apt/sources.list.d/gvisor.list
      apt-get update -q
      apt-get install -yq runsc
      changed "installed runsc"
    fi
  fi

  # Merge the runsc runtime and systemd cgroup driver into daemon.json. Other keys are kept.
  local file=/etc/docker/daemon.json
  local runsc_policy='{"exec-opts":["native.cgroupdriver=systemd"],"runtimes":{"runsc":{"path":"/usr/bin/runsc"}},"live-restore":true,"log-driver":"local"}'
  if ! have jq; then
    note "jq is missing, so $file is not checked (the packages step installs it)"
    return 0
  fi
  local current="{}" merged normal
  if [ -f "$file" ]; then
    current="$(cat "$file")"
    printf '%s' "$current" | jq -e . >/dev/null || die "$file is not valid JSON; fix it by hand"
  fi
  merged="$(printf '%s' "$current" | jq -S --argjson want "$runsc_policy" '. * $want')"
  normal="$(printf '%s' "$current" | jq -S .)"
  if [ "$merged" = "$normal" ]; then
    note "unchanged: $file already has the runsc runtime and systemd cgroup driver"
    return 0
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    note "would write $file (merge runsc runtime, systemd cgroup driver) and restart docker"
    return 0
  fi
  install -d -m 0755 /etc/docker
  printf '%s\n' "$merged" > "$file.tmp"
  mv -f "$file.tmp" "$file"
  systemctl enable docker >/dev/null 2>&1 || true
  systemctl restart docker
  changed "wrote $file and restarted docker (live-restore keeps running containers)"
}

ensure_rust() {
  if have cargo; then return 0; fi
  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    return 0
  fi
  (
    dir="$(mktemp -d)"
    trap 'rm -rf "$dir"' EXIT
    base=https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu
    curl -fsSL "$base/rustup-init" -o "$dir/rustup-init"
    curl -fsSL "$base/rustup-init.sha256" -o "$dir/rustup-init.sha256"
    (cd "$dir" && sed 's#\*.*#*rustup-init#' rustup-init.sha256 | sha256sum -c)
    chmod +x "$dir/rustup-init"
    "$dir/rustup-init" -y --profile minimal --default-toolchain 1.90.0 --no-modify-path
  )
  export PATH="$HOME/.cargo/bin:$PATH"
  changed "installed rustup and the Rust toolchain from rust-toolchain.toml"
}

build_binary() {
  step "agent-rush binary"
  local src=""
  if [ -n "${AGENT_RUSH_BINARY:-}" ]; then
    [ -x "$AGENT_RUSH_BINARY" ] || die "AGENT_RUSH_BINARY is not an executable file"
    src="$AGENT_RUSH_BINARY"
    note "using the prebuilt binary named by AGENT_RUSH_BINARY"
  elif [ -x "$REPO/target/release/agent-rush" ] \
    && [ -z "$(find "$REPO/src" "$REPO/Cargo.toml" "$REPO/Cargo.lock" -newer "$REPO/target/release/agent-rush" -print -quit)" ]; then
    src="$REPO/target/release/agent-rush"
    note "reusing target/release/agent-rush (newer than every source file)"
  else
    if [ "$DRY_RUN" -eq 1 ]; then
      note "would install rustup if missing, then build from source"
      note "would run: cargo build --locked --release"
      [ "${AR_SKIP_TESTS:-0}" = 1 ] || note "would run: cargo test --locked"
    else
      ensure_rust
      (cd "$REPO" && cargo build --locked --release)
      if [ "${AR_SKIP_TESTS:-0}" != 1 ]; then
        (cd "$REPO" && cargo test --locked)
      fi
      src="$REPO/target/release/agent-rush"
      changed "built target/release/agent-rush"
    fi
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    note "would install $BIN if it differs"
    return 0
  fi
  "$src" --version >/dev/null
  install -d -m 0755 "$PREFIX"
  if [ -f "$BIN" ] && cmp -s "$src" "$BIN"; then
    note "unchanged: $BIN"
  else
    install -m 0755 "$src" "$BIN"
    changed "installed $BIN"
    RESTART_WORKER=1
    RESTART_CONTROL=1
  fi
}

accounts() {
  step "Service user and directories"
  if id -u agent-rush >/dev/null 2>&1; then
    note "exists: user agent-rush"
  else
    act useradd --system --home-dir "$DATA" --shell /usr/sbin/nologin agent-rush
    changed "created user agent-rush"
  fi
  if [ "$DO_WORKER" -eq 1 ]; then
    if id -u agent-rush >/dev/null 2>&1; then
      case " $(id -nG agent-rush) " in
        *" docker "*) note "exists: agent-rush is in the docker group" ;;
        *) act usermod -aG docker agent-rush ;;
      esac
    fi
  fi
  act install -d -m 0755 "$PREFIX"
  act install -d -m 0750 "$ETC" "$ETC/tls"
  act install -d -o agent-rush -g agent-rush -m 0700 "$DATA"
  if [ "$DO_CONTROL" -eq 1 ]; then act install -d -o agent-rush -g agent-rush -m 0700 "$DATA/control"; fi
  if [ "$DO_WORKER" -eq 1 ]; then act install -d -o agent-rush -g agent-rush -m 0700 "$DATA/worker"; fi
}

env_files() {
  step "Configuration files (mode 0600; secrets generated once)"
  local worker_env="$ETC/worker.env" control_env="$ETC/control.env" mark

  if [ "$DO_WORKER" -eq 1 ]; then
    mark=${#CHANGES[@]}
    env_file_from_example "$worker_env" "$REPO/deploy/worker.env.example"
    env_set "$worker_env" AR_WORKER_TOKEN "$(gen_hex 32)" fill
    env_set "$worker_env" AR_SANDBOX_IMAGE "$SANDBOX_IMAGE" fill
    if [ "${#CHANGES[@]}" -gt "$mark" ]; then RESTART_WORKER=1; fi
  fi

  if [ "$DO_CONTROL" -eq 1 ]; then
    mark=${#CHANGES[@]}
    env_file_from_example "$control_env" "$REPO/deploy/control.env.example"

    local mode=fill
    [ -n "${AR_SETUP_PROVIDER:-}" ] && mode=force
    env_set "$control_env" AR_INFERENCE_PROVIDER "${AR_SETUP_PROVIDER:-vultr}" "$mode"
    if [ -n "${AR_SETUP_INFERENCE_KEY:-}" ]; then
      if [ "${AR_SETUP_PROVIDER:-vultr}" = anthropic ]; then
        env_set "$control_env" ANTHROPIC_API_KEY "$AR_SETUP_INFERENCE_KEY" force
      else
        env_set "$control_env" VULTR_INFERENCE_API_KEY "$AR_SETUP_INFERENCE_KEY" force
      fi
    fi
    env_set "$control_env" AR_OPERATOR_PASSWORD "${AR_SETUP_ADMIN_PASSWORD:-$(gen_hex 36)}" fill

    if [ "$ROLE" = all ]; then
      env_set "$control_env" AR_WORKER_URL "http://127.0.0.1:$WORKER_PORT" force
      env_set "$control_env" AR_WORKER_TOKEN "$(env_get "$worker_env" AR_WORKER_TOKEN)" fill
    else
      env_set "$control_env" AR_WORKER_URL "$WORKER_URL_SPLIT" force
      if [ -n "${AR_WORKER_TOKEN:-}" ]; then
        env_set "$control_env" AR_WORKER_TOKEN "$AR_WORKER_TOKEN" force
      elif [ -z "$(env_get "$control_env" AR_WORKER_TOKEN)" ]; then
        note "AR_WORKER_TOKEN is empty: copy the worker's token into $control_env before starting control"
      fi
    fi
    if has_feature voice-stt || has_feature voice-tts; then
      env_set "$control_env" AR_VOICE_URL "http://127.0.0.1:8790" force
    fi
    if [ "${#CHANGES[@]}" -gt "$mark" ]; then RESTART_CONTROL=1; fi
  fi
}

box_host() {
  step "Worker: box network, sandbox image and workspace image"
  if [ -f "$REPO/deploy/box-network.sh" ]; then
    act bash "$REPO/deploy/box-network.sh"
    note "ran deploy/box-network.sh (egress filter for boxes)"
  else
    note "deploy/box-network.sh is not in this checkout: the box egress filter is not applied"
  fi

  if [ "$DRY_RUN" -eq 0 ] && docker image inspect "$SANDBOX_IMAGE" >/dev/null 2>&1; then
    note "present: sandbox image (pinned digest)"
  else
    act docker pull "$SANDBOX_IMAGE"
    changed "pulled the pinned sandbox image"
  fi

  if has_feature browser; then
    if grep -q -- '--workspace' "$REPO/scripts/build-images.sh"; then
      act "$REPO/scripts/build-images.sh" --workspace
      changed "built the workspace image"
    else
      note "browser: scripts/build-images.sh has no workspace target in this checkout; not built"
    fi
  fi
}

run_as_worker() {
  # shellcheck source=/dev/null
  ( set -a; . "$ETC/worker.env"; set +a; runuser -u agent-rush -- "$BIN" "$@" )
}

worker_qualify() {
  step "Worker: qualification (real containment checks)"
  local report="$DATA/qualification.json" next="$DATA/qualification.next.json"
  if [ "$DRY_RUN" -eq 1 ]; then
    note "would run: agent-rush qualify (writes $report)"
    note "would run: agent-rush qualify-box, if this binary provides it"
    return 0
  fi
  rm -f "$next"
  if run_as_worker qualify --output "$next"; then
    mv -f "$next" "$report"
    changed "wrote the worker qualification report"
  else
    die "qualification failed: the worker is not started. Read the output above, fix the host, then run install again."
  fi

  local help
  help="$("$BIN" --help 2>&1 || true)"
  case "$help" in
    *qualify-box*)
      if run_as_worker qualify-box --output "$DATA/box-qualification.json"; then
        changed "wrote the box qualification report"
      else
        die "box qualification failed: the worker is not started. Read the output above, then run install again."
      fi
      ;;
    *) note "agent-rush qualify-box is not in this binary: box qualification skipped" ;;
  esac
}

worker_services() {
  step "Worker: systemd units and requalification timer"
  install_file "$REPO/deploy/worker.service" "$UNITS/agent-rush-worker.service" 0644
  if [ "$LAST_CHANGED" -eq 1 ]; then RESTART_WORKER=1; fi
  install_file "$REPO/deploy/requalify.sh" "$PREFIX/requalify.sh" 0755
  install_file "$REPO/deploy/agent-rush-requalify.service" "$UNITS/agent-rush-requalify.service" 0644
  install_file "$REPO/deploy/agent-rush-requalify.timer" "$UNITS/agent-rush-requalify.timer" 0644
  act systemctl daemon-reload
  act systemctl enable agent-rush-requalify.timer
  act systemctl start agent-rush-requalify.timer
  act systemctl enable agent-rush-worker
  if [ "$DRY_RUN" -eq 0 ] && [ "$RESTART_WORKER" -eq 1 ] && systemctl is-active --quiet agent-rush-worker; then
    note "restarting agent-rush-worker: in-flight sandbox runs stop"
    act systemctl restart agent-rush-worker
  else
    act systemctl start agent-rush-worker
  fi
  if [ "$DRY_RUN" -eq 0 ]; then changed "worker service enabled and running"; fi
}

control_services() {
  step "Control: systemd unit"
  install_file "$REPO/deploy/control.service" "$UNITS/agent-rush-control.service" 0644
  if [ "$LAST_CHANGED" -eq 1 ]; then RESTART_CONTROL=1; fi
  act systemctl daemon-reload
  act systemctl enable agent-rush-control
  if [ "$DRY_RUN" -eq 0 ] && [ "$RESTART_CONTROL" -eq 1 ] && systemctl is-active --quiet agent-rush-control; then
    note "restarting agent-rush-control"
    act systemctl restart agent-rush-control
  else
    act systemctl start agent-rush-control
  fi
  if [ "$DRY_RUN" -eq 0 ]; then
    local bind port
    bind="$(env_get "$ETC/control.env" AR_BIND)"
    port="${bind##*:}"
    for _ in $(seq 1 30); do
      curl -fsS "http://127.0.0.1:${port:-8080}/healthz" >/dev/null 2>&1 && break
      sleep 1
    done
    if curl -fsS "http://127.0.0.1:${port:-8080}/healthz" >/dev/null 2>&1; then
      note "healthz on 127.0.0.1:${port:-8080}: ok"
    else
      die "control did not answer on 127.0.0.1:${port:-8080}; see: journalctl -u agent-rush-control"
    fi
  fi
}

split_hosts_entry() {
  if [ -z "${AR_WORKER_PRIVATE_IP:-}" ]; then return 0; fi
  step "Control: internal name for the worker"
  if [ "$DRY_RUN" -eq 1 ]; then
    note "would point worker.agent-rush.internal at $AR_WORKER_PRIVATE_IP in /etc/hosts"
    return 0
  fi
  if grep -q "^$AR_WORKER_PRIVATE_IP worker.agent-rush.internal\$" /etc/hosts; then
    note "unchanged: /etc/hosts"
    return 0
  fi
  sed -i '/worker.agent-rush.internal/d' /etc/hosts
  echo "$AR_WORKER_PRIVATE_IP worker.agent-rush.internal" >> /etc/hosts
  changed "/etc/hosts maps worker.agent-rush.internal to the worker's private address"
}

tls_front() {
  step "Control: public TLS with Caddy and an sslip.io name"
  local ip="${AR_PUBLIC_IP:-}"
  if [ -z "$ip" ] && [ "$DRY_RUN" -eq 0 ]; then
    ip="$(curl -fsS --max-time 10 https://api.ipify.org || true)"
  fi
  if [ -z "$ip" ] && [ "$DRY_RUN" -eq 1 ]; then
    note "would detect the public IPv4 (set AR_PUBLIC_IP to skip detection)"
    ip="A.B.C.D"
  fi
  if ! [[ "$ip" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] && [ "$ip" != "A.B.C.D" ]; then
    die "could not detect a public IPv4 address; set AR_PUBLIC_IP"
  fi
  local host="${ip//./-}.sslip.io" port
  PUBLIC_HOST="$host"
  port="$(env_get "$ETC/control.env" AR_BIND)"
  port="${port##*:}"
  port="${port:-8080}"
  note "public name: https://$host (Caddy obtains the certificate on first request)"

  if have caddy; then
    note "present: caddy"
  elif [ "$DRY_RUN" -eq 1 ]; then
    note "would install caddy from its signed apt repository"
  else
    curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
    curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt > /etc/apt/sources.list.d/caddy-stable.list
    apt-get update -q
    apt-get install -yq caddy
    changed "installed caddy"
  fi

  local caddyfile
  caddyfile="$(mktemp)"
  cat > "$caddyfile" <<CADDY
$host {
	encode gzip
	header {
		Strict-Transport-Security "max-age=31536000"
		X-Content-Type-Options nosniff
		Referrer-Policy no-referrer
		-Server
	}
	reverse_proxy 127.0.0.1:$port
}
CADDY
  if [ "$DRY_RUN" -eq 1 ]; then
    note "would write /etc/caddy/Caddyfile for $host"
    rm -f "$caddyfile"
  else
    install_file "$caddyfile" /etc/caddy/Caddyfile 0644
    rm -f "$caddyfile"
    if [ "$LAST_CHANGED" -eq 1 ]; then systemctl reload-or-restart caddy; fi
  fi

  local ufw_status=""
  if have ufw; then ufw_status="$(ufw status 2>/dev/null || true)"; fi
  case "$ufw_status" in
    *"Status: active"*)
      act ufw allow 80/tcp
      act ufw allow 443/tcp
      ;;
    *) note "ufw is not active: open ports 80 and 443 in the cloud firewall" ;;
  esac
}

voice_service() {
  if ! has_feature voice-stt && ! has_feature voice-tts; then return 0; fi
  step "Voice service (optional, localhost only)"
  local -a flags=()
  if has_feature voice-stt; then flags+=(--stt); fi
  if has_feature voice-tts; then flags+=(--tts); fi
  act bash "$REPO/voice/install.sh" "${flags[@]}" --device cpu --systemd
  changed "voice service installed (127.0.0.1:8790)"
}

summary() {
  step "Summary"
  if [ "$DRY_RUN" -eq 1 ]; then
    say "Dry run: nothing was changed. Re-run without --dry-run to apply this plan."
    return 0
  fi
  if [ "${#CHANGES[@]}" -eq 0 ]; then
    say "Nothing changed: the host already matched the plan."
  else
    say "Changes:"
    local c
    for c in "${CHANGES[@]}"; do say "  - $c"; done
  fi
  say ""
  if [ "$DO_CONTROL" -eq 1 ]; then
    if has_feature tls; then
      say "Open the control plane at https://${PUBLIC_HOST:-<public-name>}"
    else
      say "Open the control plane through an SSH tunnel: ssh -L 8080:127.0.0.1:8080 <this-host>"
    fi
    say "Admin password (keep it private): sudo grep '^AR_OPERATOR_PASSWORD=' $ETC/control.env"
  fi
  if [ "$DO_WORKER" -eq 1 ]; then
    say "Worker status: systemctl status agent-rush-worker"
  fi
  say "Uninstall: see docs/INSTALL.md."
}

main() {
  parse_args "$@"
  say "Agent Rush install: role=$ROLE features=${WITH:-none}$([ "$DRY_RUN" -eq 1 ] && echo ' (dry run)')"
  preflight
  confirm

  packages
  if [ "$DO_WORKER" -eq 1 ]; then docker_runsc; fi
  build_binary
  accounts
  env_files
  if [ "$DO_WORKER" -eq 1 ]; then
    box_host
    worker_qualify
    worker_services
  fi
  if [ "$DO_CONTROL" -eq 1 ]; then
    split_hosts_entry
    control_services
    if has_feature tls; then tls_front; fi
    voice_service
  fi
  summary
}

main "$@"
