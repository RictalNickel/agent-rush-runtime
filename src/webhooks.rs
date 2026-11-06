//! Signed webhooks: on each turn end (status `idle` or `failed`) the owner's webhooks get
//! `POST {session_id, status, last_reply, ...}` with `X-Agent-Signature: sha256=<hex>`,
//! the HMAC-SHA256 of the exact body under the webhook's secret.
//!
//! Webhook URLs are operator-facing input, so the control plane refuses any URL whose
//! host resolves to a private, loopback, link-local or otherwise non-public address, both
//! when the webhook is created and again at every delivery. Each delivery connects to the
//! address it checked (no second DNS lookup), ignores proxy settings from the environment
//! and never follows redirects.
use crate::{ledger::Ledger, session::Sessions, session_store::SessionEvent};
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::sync::broadcast::error::RecvError;

pub const SIGNATURE_HEADER: &str = "x-agent-signature";
pub const DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);
/// Attempts after the first one.
pub const RETRIES: u32 = 2;
const MAX_REPLY_BYTES: usize = 16 * 1024;

/// HMAC-SHA256 (RFC 2104) with the existing `sha2` crate.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

/// The `X-Agent-Signature` value for a body: `sha256=<hex>`.
pub fn signature(secret: &str, body: &[u8]) -> String {
    format!(
        "sha256={}",
        hex::encode(hmac_sha256(secret.as_bytes(), body))
    )
}

/// True for addresses a webhook must never reach: loopback, private, link-local,
/// carrier-grade NAT, multicast, documentation, reserved and unspecified ranges.
pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => forbidden_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return forbidden_v4(v4);
            }
            let s = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link-local
                || (s[0] & 0xffc0) == 0xfec0 // site-local (deprecated)
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || (s[0] == 0x0064 && s[1] == 0xff9b) // NAT64
                || (s[0] == 0x2002) // 6to4 can embed private IPv4
                || (s[0..6] == [0, 0, 0, 0, 0, 0]) // IPv4-compatible
        }
    }
}
fn forbidden_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || (o[0] == 100 && (64..128).contains(&o[1])) // carrier-grade NAT
        || (o[0] == 192 && o[1] == 0 && o[2] == 0) // IETF protocol assignments
        || (o[0] == 198 && (18..20).contains(&o[1])) // benchmarking
        || o[0] >= 240 // reserved
}

/// Which webhook destinations are allowed. Production uses `public()`; only tests may
/// use the loopback exception, through its explicitly named constructor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UrlGuard {
    allow_loopback: bool,
}
impl Default for UrlGuard {
    fn default() -> Self {
        Self::public()
    }
}
impl UrlGuard {
    /// HTTPS only, public addresses only.
    pub fn public() -> Self {
        Self {
            allow_loopback: false,
        }
    }
    /// Test-only: also allows plain HTTP to loopback addresses, so a test can run an
    /// in-process receiver. Every other private range stays refused.
    pub fn allow_loopback_for_tests() -> Self {
        Self {
            allow_loopback: true,
        }
    }
    fn allowed(&self, ip: IpAddr) -> bool {
        !is_forbidden_ip(ip) || (self.allow_loopback && ip.is_loopback())
    }
    /// Parses and checks a webhook URL, resolving its host. Returns the URL and the
    /// address to connect to.
    pub async fn check(&self, raw: &str) -> Result<(reqwest::Url, SocketAddr)> {
        ensure!(raw.len() <= 2048, "webhook URL must be at most 2048 bytes");
        let url = reqwest::Url::parse(raw.trim()).context("invalid webhook URL")?;
        ensure!(
            url.scheme() == "https" || (self.allow_loopback && url.scheme() == "http"),
            "webhook URL must use https"
        );
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "webhook URL must not embed credentials"
        );
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("webhook URL needs a host"))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port = url
            .port_or_known_default()
            .ok_or_else(|| anyhow!("webhook URL needs a port"))?;
        let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, port)],
            Err(_) => tokio::time::timeout(
                Duration::from_secs(5),
                tokio::net::lookup_host((host.as_str(), port)),
            )
            .await
            .map_err(|_| anyhow!("resolving the webhook host timed out"))?
            .map_err(|_| anyhow!("could not resolve the webhook host"))?
            .collect(),
        };
        ensure!(!addrs.is_empty(), "could not resolve the webhook host");
        // Every address must be public, so DNS answers cannot mix in a private target.
        if addrs.iter().any(|a| !self.allowed(a.ip())) {
            bail!("webhook URL resolves to a private or reserved address");
        }
        Ok((url, addrs[0]))
    }
}

/// The client for one delivery: it connects only to `addr`, the address `UrlGuard::check`
/// approved. `no_proxy` matters: a proxy from the environment (`HTTPS_PROXY`,
/// `ALL_PROXY`, ...) would make the proxy resolve and connect to the host itself, which
/// skips the pinned address and so the private-address check.
fn delivery_client(host: &str, addr: SocketAddr) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(DELIVERY_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .resolve(host, addr)
        .build()?)
}

/// Sends one signed body, retrying twice. Re-checks the URL before each attempt.
pub async fn deliver(guard: UrlGuard, url: &str, secret: &str, body: &[u8]) -> Result<()> {
    let signature = signature(secret, body);
    let mut last = anyhow!("not sent");
    for attempt in 0..=RETRIES {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
        }
        let (parsed, addr) = match guard.check(url).await {
            Ok(target) => target,
            Err(e) => return Err(e),
        };
        let host = parsed.host_str().unwrap_or_default().to_string();
        let client = delivery_client(&host, addr)?;
        match client
            .post(parsed)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::USER_AGENT, "agent-rush-webhooks")
            .header(SIGNATURE_HEADER, &signature)
            .body(body.to_vec())
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => last = anyhow!("receiver returned {}", response.status().as_u16()),
            Err(e) => last = anyhow!("delivery failed: {}", e.without_url()),
        }
    }
    Err(last)
}

/// The webhook body for a turn-end status event, or None for other events.
pub fn turn_end_payload(ledger: &Ledger, event: &SessionEvent) -> Option<Value> {
    if event.kind != "status" {
        return None;
    }
    let status = event.payload["status"].as_str()?;
    if !matches!(status, "idle" | "failed") {
        return None;
    }
    // The last assistant reply of this turn: scan back to the turn's `running` status.
    let mut last_reply = Value::Null;
    if let Ok(events) = ledger.recent_session_events(&event.session_id, 200) {
        for e in events.iter().rev().filter(|e| e.seq <= event.seq) {
            if e.kind == "assistant" {
                let text = e.payload["text"].as_str().unwrap_or("");
                last_reply = json!(crate::agent::clip(text, MAX_REPLY_BYTES));
                break;
            }
            if e.kind == "status" && e.payload["status"] == "running" {
                break;
            }
        }
    }
    Some(json!({
        "event": "turn.end",
        "session_id": event.session_id,
        "status": status,
        "reason": event.payload["reason"],
        "last_reply": last_reply,
        "seq": event.seq,
        "at": event.at,
    }))
}

/// The background task: follows every session event and delivers turn ends.
pub fn spawn(
    sessions: Sessions,
    ledger: Arc<Ledger>,
    guard: UrlGuard,
) -> tokio::task::JoinHandle<()> {
    let mut events = sessions.subscribe();
    tokio::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(
                        skipped = n,
                        "webhook dispatcher lagged; some turn ends were not delivered"
                    );
                    continue;
                }
                Err(RecvError::Closed) => return,
            };
            let Some(payload) = turn_end_payload(&ledger, &event) else {
                continue;
            };
            let Ok(Some(owner)) = ledger.agent_session_owner(&event.session_id) else {
                continue;
            };
            let hooks = match ledger.webhooks_with_secrets(&owner) {
                Ok(hooks) => hooks,
                Err(e) => {
                    tracing::warn!("listing webhooks failed: {e:#}");
                    continue;
                }
            };
            let body = payload.to_string().into_bytes();
            for hook in hooks {
                let body = body.clone();
                tokio::spawn(async move {
                    if let Err(e) = deliver(guard, &hook.webhook.url, &hook.secret, &body).await {
                        tracing::warn!(webhook = %hook.webhook.id, "webhook delivery failed: {e:#}");
                    }
                });
            }
        }
    })
}
