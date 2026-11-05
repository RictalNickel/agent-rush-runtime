#!/usr/bin/env bash
# Install the private execution node on a bootstrapped worker (see bootstrap-worker.sh).
# Expects, uploaded by the trusted deployment runner:
#   /root/agent-rush/target/release/agent-rush   release binary (built on this host)
#   /root/deploy-in/worker.env                   AR_WORKER_TOKEN etc. (mode 0600)
#   /root/deploy-in/tls/{ca.crt,worker.crt,worker.key}
# Usage: install-worker.sh <private-vpc-ip> <control-private-ip>
set -euo pipefail
VPC_IP="${1:?private VPC IP required}"
CONTROL_IP="${2:?control private IP required}"
IMAGE="rust:1.90.0-bookworm@sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f"
export DEBIAN_FRONTEND=noninteractive
apt-get install -yq nginx

id agent-rush >/dev/null 2>&1 || useradd --system --home /var/lib/agent-rush --shell /usr/sbin/nologin agent-rush
usermod -aG docker agent-rush
install -d -o agent-rush -g agent-rush -m 0700 /var/lib/agent-rush /var/lib/agent-rush/worker
install -d -m 0755 /opt/agent-rush
install -m 0755 /root/agent-rush/target/release/agent-rush /opt/agent-rush/agent-rush
install -d -m 0750 /etc/agent-rush /etc/agent-rush/tls
install -m 0600 /root/deploy-in/worker.env /etc/agent-rush/worker.env
grep -q '^AR_SANDBOX_IMAGE=' /etc/agent-rush/worker.env || echo "AR_SANDBOX_IMAGE=$IMAGE" >> /etc/agent-rush/worker.env
install -m 0644 /root/deploy-in/tls/ca.crt /root/deploy-in/tls/worker.crt /etc/agent-rush/tls/
install -m 0600 /root/deploy-in/tls/worker.key /etc/agent-rush/tls/

install -m 0644 /root/agent-rush/deploy/worker.service /etc/systemd/system/agent-rush-worker.service

# Requalify on a timer: the worker refuses work once its report is two hours old.
# A new report is installed only when every check passes; the running worker adopts it
# within a minute, so in-flight sandbox runs are never interrupted by a restart.
cat > /opt/agent-rush/requalify.sh <<'SH'
#!/usr/bin/env bash
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
SH
chmod 0755 /opt/agent-rush/requalify.sh
cat > /etc/systemd/system/agent-rush-requalify.service <<'UNIT'
[Unit]
Description=Agent Rush worker containment requalification
After=docker.service
[Service]
Type=oneshot
ExecStart=/opt/agent-rush/requalify.sh
UNIT
cat > /etc/systemd/system/agent-rush-requalify.timer <<'UNIT'
[Unit]
Description=Requalify the Agent Rush worker every 45 minutes (reports last two hours)
[Timer]
OnBootSec=2min
OnUnitActiveSec=45min
[Install]
WantedBy=timers.target
UNIT

# mTLS ingress bound to the private VPC address only.
sed -e "s/PRIVATE_WORKER_IP/$VPC_IP/" -e "s/PRIVATE_WORKER_NAME/worker.agent-rush.internal/" \
  -e '/^# /d' /root/agent-rush/deploy/worker-nginx.conf.example > /etc/nginx/sites-available/agent-rush-worker
ln -sf /etc/nginx/sites-available/agent-rush-worker /etc/nginx/sites-enabled/agent-rush-worker
rm -f /etc/nginx/sites-enabled/default
nginx -t
# Only the control VM may reach the mTLS ingress, and only over the private network.
ufw allow from "$CONTROL_IP" to "$VPC_IP" port 8443 proto tcp
systemctl daemon-reload
systemctl enable agent-rush-requalify.timer
# One synchronous qualification before first start; the timer then keeps it fresh.
systemctl start agent-rush-requalify.service
systemctl start agent-rush-requalify.timer
systemctl enable agent-rush-worker
systemctl restart agent-rush-worker
systemctl reload-or-restart nginx
sleep 2
systemctl --no-pager --lines=5 status agent-rush-worker | head -12
