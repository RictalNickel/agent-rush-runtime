# Install

Boxoro runs on Ubuntu 24.04 (x86-64) with cgroup v2 and root access. The installer is tested on that system only.

## Quick install

One host, both roles, no extra features:

```sh
sudo scripts/install.sh --role all --yes
```

Preview first. This changes nothing:

```sh
scripts/install.sh --dry-run --role all --with browser,voice-stt
```

Guided setup (asks for the role, features, provider and key):

```sh
sudo scripts/setup.sh
```

The installer is idempotent. Run it again after an update, and it reports what it changed.

## Pick features

Use `--with` with a comma list, or pick them in `setup.sh`.

| Feature | Role | What it adds |
| --- | --- | --- |
| `browser` | worker, all | Workspace image with a shell, browser and file converters for agent boxes. Built by `scripts/build-images.sh --workspace` once that target exists. |
| `voice-stt` | control, all | Speech to text (Moonshine, CPU). See [VOICE.md](VOICE.md). |
| `voice-tts` | control, all | Speech output (LuxTTS, CPU). Needs a reference clip. See [VOICE.md](VOICE.md). |
| `tls` | control, all | Public HTTPS with Caddy on an `sslip.io` name. Needs ports 80 and 443 open. |
| `mcp` | setup only | Writes an example MCP client config with placeholders. It holds no key. |

Inference runs on Vultr Serverless Inference by default. `setup.sh --provider anthropic` selects the development provider.

## Roles

- **`all`** (default): control and worker on one host. This is a weaker separation than two VMs. The control plane and the sandboxes share one kernel and one disk. Use it for development and demos.
- **`control`**: the public control plane. Set `AR_WORKER_TOKEN` to the worker's token, and `AR_WORKER_PRIVATE_IP` to the worker's private address. Place the mTLS files `ca.crt` and `control-client.pem` in `/etc/agent-rush/tls` first. See [DEPLOYMENT.md](DEPLOYMENT.md).
- **`worker`**: the private execution node. It installs the box firewall unit (`deploy/box-network.sh`, reapplied at every boot), builds and pins the workspace image, runs both qualifications, and starts the worker service. The installer does not apply the private mTLS ingress. Apply `deploy/worker-nginx.conf.example` by hand after the mTLS tests pass.

## What gets installed

- Packages: `docker.io`, `gVisor` (`runsc`), and the build tools. The Rust 1.90.0 toolchain comes from `rust-toolchain.toml` if you build from source. Caddy is added with `tls`.
- Binary: `/opt/agent-rush/agent-rush`. Set `AGENT_RUSH_BINARY` to a prebuilt file to skip the build.
- Docker: `runsc` is added to `/etc/docker/daemon.json`, with the systemd cgroup driver. Other keys are kept.
- Configuration: `/etc/agent-rush/worker.env` and `/etc/agent-rush/control.env`, mode 0600. Secrets are generated once and never printed.
- Data: `/var/lib/agent-rush`, owned by the `agent-rush` user.
- Services: `agent-rush-worker`, `agent-rush-requalify.timer` (reruns the containment checks every 45 minutes), and `agent-rush-control`.
- Qualification: `agent-rush qualify` writes `/var/lib/agent-rush/qualification.json`. The worker does not start unless it passes.
- Optional: Caddy and `/etc/caddy/Caddyfile` with `tls`, and the voice service with `voice-stt` or `voice-tts`.

Read the admin password with:

```sh
sudo grep '^AR_OPERATOR_PASSWORD=' /etc/agent-rush/control.env
```

Restarts happen only when the binary, a unit or a relevant env file changed. Restarting the worker stops sandbox runs that are in flight.

## Uninstall

The installer has no uninstall step yet. Back up `/var/lib/agent-rush` first, because it holds the ledger and every session.

```sh
sudo systemctl disable --now agent-rush-control agent-rush-worker agent-rush-requalify.timer agent-voice
sudo rm -f /etc/systemd/system/agent-rush-*.service /etc/systemd/system/agent-rush-requalify.timer /etc/systemd/system/agent-voice.service
sudo systemctl daemon-reload
sudo rm -rf /opt/agent-rush /opt/agent-voice /etc/agent-rush /var/lib/agent-rush
```

Docker and `runsc` stay installed. Remove them with `apt` if you no longer need them.

## Troubleshooting

- **Qualification failed.** The worker stays stopped. Run `sudo -u agent-rush /opt/agent-rush/agent-rush qualify --output /tmp/q.json` and read the failing check. Common causes: Docker is not running, `runsc` is not in `docker info`, or cgroup v2 is missing.
- **Control does not answer on 127.0.0.1:8080.** Run `journalctl -u agent-rush-control -n 50`. A bad value in `control.env` stops the service at start.
- **Control reports 401 to the worker.** `AR_WORKER_TOKEN` in `control.env` must equal the token in the worker's `worker.env`.
- **No certificate with `tls`.** The `sslip.io` name must resolve to this host's public IP, and ports 80 and 443 must be open in the cloud firewall. Caddy logs are in `journalctl -u caddy`.
- **Voice returns 501.** Check `curl -s http://127.0.0.1:8790/health`. Put a reference clip at `/opt/agent-voice/ref.wav` and restart `agent-voice`. Logs are in `journalctl -u agent-voice`.
- **Unsure what a run will change.** Add `--dry-run`. It reads the host and prints the plan.
