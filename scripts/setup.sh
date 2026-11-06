#!/usr/bin/env bash
# Guided setup: pick a role and features, enter the inference key and admin password,
# then run scripts/install.sh with those choices.
#
# Usage: scripts/setup.sh [--dry-run] [--yes] [--role all|control|worker]
#                         [--with LIST] [--provider vultr|anthropic]
#   --dry-run    show the plan and what install.sh would change. Asks for no secrets, changes nothing.
#   --yes        skip the final confirmation (needed without a terminal)
#   --role       all (default), control or worker
#   --with       comma list: browser, voice-stt, voice-tts, tls, mcp
#   --provider   vultr (default; the Blast Radius Zero track) or anthropic (development)
#
# With a terminal, whiptail gives menus when it is installed; otherwise numbered prompts are used.
# Without a terminal, the flags and defaults decide, and AR_SETUP_INFERENCE_KEY supplies the key.
# The key is read without echo and reaches only the 0600 env file written by install.sh.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ETC=/etc/agent-rush

ROLE=""
WITH=""
PROVIDER=""
DRY_RUN=0
ASSUME_YES=0
INTERACTIVE=0
HAVE_WHIPTAIL=0
FEATURES=()
INFERENCE_KEY=""
ADMIN_PASSWORD=""

say() { printf '%s\n' "$*"; }
die() { printf 'setup: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --dry-run) DRY_RUN=1 ;;
      --yes) ASSUME_YES=1 ;;
      --role) ROLE="${2:?--role needs a value}"; shift ;;
      --role=*) ROLE="${1#--role=}" ;;
      --with) WITH="${2:?--with needs a comma list}"; shift ;;
      --with=*) WITH="${1#--with=}" ;;
      --provider) PROVIDER="${2:?--provider needs vultr or anthropic}"; shift ;;
      --provider=*) PROVIDER="${1#--provider=}" ;;
      -h | --help) sed -n '2,16p' "$0"; exit 0 ;;
      *) die "unknown option: $1 (see --help)" ;;
    esac
    shift
  done
  if [ -t 0 ] && [ -t 1 ]; then INTERACTIVE=1; fi
  if have whiptail && [ "$INTERACTIVE" -eq 1 ]; then HAVE_WHIPTAIL=1; fi
}

choose_role() {
  if [ -n "$ROLE" ]; then
    case "$ROLE" in all | control | worker) return 0 ;; *) die "--role must be all, control or worker" ;; esac
  fi
  ROLE=all
  [ "$INTERACTIVE" -eq 1 ] || return 0
  if [ "$HAVE_WHIPTAIL" -eq 1 ]; then
    ROLE="$(whiptail --title 'Vultr-Placeholder-Test123 setup' --menu 'Where should this install run?' 16 74 3 \
      all 'Both roles on this host (quick; weaker separation than two VMs)' \
      control 'Public control plane only' \
      worker 'Private execution node only' 3>&1 1>&2 2>&3)" || die "cancelled"
  else
    say "Where should this install run?"
    say "  1) all      both roles on this host (quick; weaker separation than two VMs)"
    say "  2) control  public control plane only"
    say "  3) worker   private execution node only"
    local answer
    read -r -p "Choose 1-3 [1]: " answer
    case "${answer:-1}" in
      1) ROLE=all ;;
      2) ROLE=control ;;
      3) ROLE=worker ;;
      *) die "not a choice: $answer" ;;
    esac
  fi
}

# The feature list is either given with --with or chosen here. Defaults depend on the role.
choose_features() {
  local want_browser=0 want_mcp=0
  if [ "$ROLE" != control ]; then want_browser=1; fi
  if [ "$ROLE" != worker ]; then want_mcp=1; fi

  if [ -n "$WITH" ]; then
    local item
    local -a given=()
    IFS=',' read -r -a given <<< "$WITH" || true
    for item in "${given[@]}"; do
      case "$item" in
        "") ;;
        browser | voice-stt | voice-tts | tls | mcp) FEATURES+=("$item") ;;
        *) die "unknown feature: $item (use browser, voice-stt, voice-tts, tls, mcp)" ;;
      esac
    done
    return 0
  fi

  if [ "$INTERACTIVE" -eq 0 ]; then
    [ "$want_browser" -eq 1 ] && FEATURES+=(browser)
    [ "$want_mcp" -eq 1 ] && FEATURES+=(mcp)
    return 0
  fi

  local picked
  if [ "$HAVE_WHIPTAIL" -eq 1 ]; then
    picked="$(whiptail --title 'Optional features' --checklist 'Space to toggle, Enter to confirm' 20 78 5 \
      browser 'Workspace image: shell, browser, converters (worker)' "$([ "$want_browser" -eq 1 ] && echo ON || echo OFF)" \
      voice-stt 'Speech to text with Moonshine (control, localhost)' OFF \
      voice-tts 'Speech output with LuxTTS (control, localhost)' OFF \
      tls 'Public HTTPS with Caddy and an sslip.io name (control)' OFF \
      mcp 'Example MCP client config (control)' "$([ "$want_mcp" -eq 1 ] && echo ON || echo OFF)" \
      3>&1 1>&2 2>&3)" || die "cancelled"
    picked="${picked//\"/}"
    local item
    for item in $picked; do FEATURES+=("$item"); done
  else
    say "Optional features (numbers separated by spaces, Enter for the default):"
    say "  1) browser    workspace image: shell, browser, converters (worker)"
    say "  2) voice-stt  speech to text with Moonshine (control, localhost)"
    say "  3) voice-tts  speech output with LuxTTS (control, localhost)"
    say "  4) tls        public HTTPS with Caddy and an sslip.io name (control)"
    say "  5) mcp        example MCP client config (control)"
    local defaults="" answer n
    [ "$want_browser" -eq 1 ] && defaults="$defaults 1"
    [ "$want_mcp" -eq 1 ] && defaults="$defaults 5"
    read -r -p "Select [${defaults# }]: " answer
    answer="${answer:-${defaults# }}"
    for n in $answer; do
      case "$n" in
        1) FEATURES+=(browser) ;;
        2) FEATURES+=(voice-stt) ;;
        3) FEATURES+=(voice-tts) ;;
        4) FEATURES+=(tls) ;;
        5) FEATURES+=(mcp) ;;
        *) die "not a choice: $n" ;;
      esac
    done
  fi
}

has() {
  local f
  for f in "${FEATURES[@]:-}"; do [ "$f" = "$1" ] && return 0; done
  return 1
}

install_flag_list() {
  local f out=""
  for f in "${FEATURES[@]:-}"; do
    case "$f" in browser | voice-stt | voice-tts | tls) out="${out:+$out,}$f" ;; esac
  done
  printf '%s' "$out"
}

choose_provider() {
  if [ -n "$PROVIDER" ]; then
    case "$PROVIDER" in vultr | anthropic) return 0 ;; *) die "--provider must be vultr or anthropic" ;; esac
  fi
  PROVIDER=vultr
  [ "$INTERACTIVE" -eq 1 ] || return 0
  if [ "$HAVE_WHIPTAIL" -eq 1 ]; then
    PROVIDER="$(whiptail --title 'Inference provider' --menu 'Which provider runs the agents?' 14 74 2 \
      vultr 'Vultr Serverless Inference (default)' \
      anthropic 'Anthropic API (development only)' 3>&1 1>&2 2>&3)" || die "cancelled"
  else
    local answer
    say "Inference provider: 1) vultr (default)  2) anthropic (development only)"
    read -r -p "Choose 1-2 [1]: " answer
    case "${answer:-1}" in
      1) PROVIDER=vultr ;;
      2) PROVIDER=anthropic ;;
      *) die "not a choice: $answer" ;;
    esac
  fi
}

read_secrets() {
  # Dry runs never ask for secrets. Without a terminal, the environment supplies them.
  if [ "$DRY_RUN" -eq 1 ]; then return 0; fi
  INFERENCE_KEY="${AR_SETUP_INFERENCE_KEY:-}"
  ADMIN_PASSWORD="${AR_SETUP_ADMIN_PASSWORD:-}"
  [ "$INTERACTIVE" -eq 1 ] || return 0

  local label="Vultr inference API key"
  if [ "$PROVIDER" = anthropic ]; then label="Anthropic API key"; fi
  if [ "$HAVE_WHIPTAIL" -eq 1 ]; then
    INFERENCE_KEY="$(whiptail --passwordbox "$label (input hidden; empty to add it later)" 10 74 3>&1 1>&2 2>&3)" || die "cancelled"
  else
    read -r -s -p "$label (input hidden; Enter to add it later): " INFERENCE_KEY
    printf '\n'
  fi

  local generate
  read -r -p "Generate the admin password? [Y/n]: " generate
  case "${generate:-Y}" in
    y | Y | yes | YES)
      ADMIN_PASSWORD=""
      ;;
    *)
      local first second
      read -r -s -p "Admin password (16 characters or more, input hidden): " first
      printf '\n'
      read -r -s -p "Repeat it: " second
      printf '\n'
      [ "$first" = "$second" ] || die "the two entries differ"
      [ "${#first}" -ge 16 ] || die "use 16 characters or more"
      ADMIN_PASSWORD="$first"
      ;;
  esac
}

show_plan() {
  local key_note="none entered: add it later to $ETC/control.env"
  local pass_note="generated by install.sh (read it with the command in its summary)"
  if [ "$DRY_RUN" -eq 1 ]; then
    key_note="asked for on a real run; not asked in a dry run"
    pass_note="generated unless you choose one on a real run"
  else
    if [ -n "$INFERENCE_KEY" ]; then key_note="entered (hidden)"; fi
    if [ -n "$ADMIN_PASSWORD" ]; then pass_note="entered (hidden)"; fi
  fi
  say ""
  say "Plan"
  say "  role:           $ROLE"
  say "  features:       ${FEATURES[*]:-none}"
  say "  provider:       $PROVIDER"
  say "  inference key:  $key_note"
  say "  admin password: $pass_note"
}

confirm_plan() {
  [ "$DRY_RUN" -eq 1 ] && return 0
  [ "$ASSUME_YES" -eq 1 ] && return 0
  if [ "$INTERACTIVE" -eq 0 ]; then die "no terminal to confirm on: pass --yes to run without one"; fi
  local answer
  read -r -p "Run the install with this plan? [y/N]: " answer
  case "$answer" in y | Y | yes | YES) ;; *) die "stopped; nothing changed" ;; esac
}

write_mcp_example() {
  local dest="$ETC/mcp-example.json"
  if [ "$DRY_RUN" -eq 1 ]; then
    say "would write $dest (placeholders only, no key)"
    return 0
  fi
  local tmp
  tmp="$(mktemp)"
  cat > "$tmp" <<'JSON'
{
  "mcpServers": {
    "agent": {
      "type": "http",
      "url": "https://<your-host>/mcp",
      "headers": { "Authorization": "Bearer <your ar_ API key>" }
    }
  }
}
JSON
  install -D -m 0644 "$tmp" "$dest"
  rm -f "$tmp"
  say "MCP example written to $dest (placeholders only)."
  say "Add it to Claude Code with: claude mcp add --transport http agent https://<your-host>/mcp --header \"Authorization: Bearer <your key>\""
}

run_install() {
  local flags
  flags="$(install_flag_list)"
  local -a args=(--role "$ROLE" --with "$flags")
  if [ "$DRY_RUN" -eq 1 ]; then
    args+=(--dry-run)
  else
    args+=(--yes)
    export AR_SETUP_PROVIDER="$PROVIDER"
    if [ -n "$INFERENCE_KEY" ]; then export AR_SETUP_INFERENCE_KEY="$INFERENCE_KEY"; fi
    if [ -n "$ADMIN_PASSWORD" ]; then export AR_SETUP_ADMIN_PASSWORD="$ADMIN_PASSWORD"; fi
  fi
  bash "$REPO/scripts/install.sh" "${args[@]}"
  if has mcp && [ "$ROLE" != worker ]; then write_mcp_example; fi
}

main() {
  parse_args "$@"
  if [ "$DRY_RUN" -eq 0 ] && [ "$(id -u)" -ne 0 ]; then
    die "run as root (sudo scripts/setup.sh), or add --dry-run"
  fi
  choose_role
  choose_features
  if [ "$ROLE" = worker ]; then
    PROVIDER="${PROVIDER:-vultr}"
  else
    choose_provider
  fi
  read_secrets
  show_plan
  confirm_plan
  run_install
}

main "$@"
