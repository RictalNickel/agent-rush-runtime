#!/usr/bin/env bash
# Install the public control plane on a fresh Ubuntu 24.04 VM.
# Expects, uploaded by the trusted deployment runner:
#   /root/deploy-in/agent-rush, /root/deploy-in/control.service
#   /root/deploy-in/control.env (mode 0600)
#   /root/deploy-in/tls/{ca.crt,control-client.pem}
# Usage: install-control.sh <public-hostname> <worker-private-ip>
set -euo pipefail
HOST="${1:?public hostname required}"
WORKER_IP="${2:?worker private IP required}"
export DEBIAN_FRONTEND=noninteractive

apt-get update -q
apt-get install -yq ca-certificates curl gnupg debian-keyring debian-archive-keyring apt-transport-https
# Caddy from its signed apt repository; it obtains and renews the public TLS certificate.
curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt > /etc/apt/sources.list.d/caddy-stable.list
apt-get update -q
apt-get install -yq caddy

id agent-rush >/dev/null 2>&1 || useradd --system --home /var/lib/agent-rush --shell /usr/sbin/nologin agent-rush
install -d -o agent-rush -g agent-rush -m 0700 /var/lib/agent-rush /var/lib/agent-rush/control
install -d -m 0755 /opt/agent-rush
install -m 0755 /root/deploy-in/agent-rush /opt/agent-rush/agent-rush
install -d -m 0750 -g agent-rush /etc/agent-rush /etc/agent-rush/tls
install -m 0600 /root/deploy-in/control.env /etc/agent-rush/control.env
install -m 0644 /root/deploy-in/tls/ca.crt /etc/agent-rush/tls/ca.crt
install -m 0640 -g agent-rush /root/deploy-in/tls/control-client.pem /etc/agent-rush/tls/control-client.pem
install -m 0644 /root/deploy-in/control.service /etc/systemd/system/agent-rush-control.service

# The worker certificate names worker.agent-rush.internal; resolve it to the private VPC address.
sed -i '/worker.agent-rush.internal/d' /etc/hosts
echo "$WORKER_IP worker.agent-rush.internal" >> /etc/hosts

cat > /etc/caddy/Caddyfile <<CADDY
$HOST {
	encode gzip
	header {
		Strict-Transport-Security "max-age=31536000"
		X-Content-Type-Options nosniff
		Referrer-Policy no-referrer
		-Server
	}
	reverse_proxy 127.0.0.1:8080
}
CADDY

# Vultr's Ubuntu image enables ufw with SSH only; the gateway needs 80 (ACME) and 443.
ufw allow 80/tcp
ufw allow 443/tcp

systemctl daemon-reload
systemctl enable --now agent-rush-control
systemctl reload-or-restart caddy
sleep 3
systemctl --no-pager --lines=8 status agent-rush-control | head -14
