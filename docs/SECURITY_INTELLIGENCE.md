# Optional security intelligence MCP

This is an optional **control-side enrichment service** for Boxoro. Local security
scanning does not require it. It provides public vulnerability context, a password
hash-prefix lookup, and optional existing-report malware reputation. It does not
scan files, upload samples or source, test credentials, check an email account, or
establish whether a system has been compromised.

Implementation: [`intel_server.py`](../deploy/security/intel_server.py).
Operator examples: [`mcp-servers.example.json`](../deploy/security/mcp-servers.example.json)
and [`security-intel.service`](../deploy/security/security-intel.service).

## Trust boundary and protocol

Run this service on the **control machine**, under its own Unix account. Never
install provider keys in the workspace image or a session box. Calls to this
service are outside the worker's gVisor sandbox and worker egress policy; the
service has its own authentication, input checks and resource limits.

The listener is fixed to `127.0.0.1`; its default endpoint is
`http://127.0.0.1:8767/mcp`. Every supported MCP request requires an exact bearer token. All
`Origin` headers are rejected, including `Origin: null`; a browser frontend is
not a supported client. The `Host` must be the loopback host and actual listening
port; omission of the port is accepted only when listening on HTTP's default
port 80. There is no CORS, public listener or unauthenticated health endpoint.

The service implements the subset of **MCP 2025-06-18 Streamable HTTP** used by
[`src/mcp_client.rs`](../src/mcp_client.rs): `initialize`,
`notifications/initialized`, `ping`, `tools/list` and `tools/call`. Responses are
JSON, initialized notifications receive HTTP 202, and no session ID is issued.
GET streaming and DELETE sessions are unsupported and return HTTP 405. It does
not implement OAuth discovery, sampling, resources or client-selected URLs.

Boxoro explicitly permits plain HTTP for a loopback MCP server. Public HTTPS
servers go through a different address guard, which rejects private addresses.
Do not change this example to an arbitrary private HTTPS address or expose it
on `0.0.0.0`. If the control plane runs in a container, the service must be in the
same trusted network namespace for this loopback configuration to work. Remote
deployment requires a separately reviewed authenticated TLS design.

MCP configuration headers are **literal values**. Boxoro does not substitute
`${SECURITY_INTEL_TOKEN}` into the JSON. Keep the populated MCP configuration out
of git, prompts, image layers and workspaces. The sample's token is only a
placeholder and will not authenticate against a separately generated secret.

## Tools and exact upstream requests

All tool argument objects reject unknown fields. The identifiers are validated
again in the separate provider process. The only upstream method is GET, using
the fixed HTTPS routes below. Environment proxy settings are ignored, normal
system certificate verification remains enabled, and redirects are refused.

| Tool | Only accepted argument | Fixed upstream | Returned information |
| --- | --- | --- | --- |
| `cve_context` | `cve`, such as `CVE-2024-12345` | `https://www.cisa.gov/sites/default/files/feeds/known_exploited_vulnerabilities.json` and `https://api.first.org/data/v1/epss?cve={cve}` | KEV membership and dates, ransomware-use category if present; EPSS probability, percentile and score date |
| `osv_vulnerability` | `id`, an `OSV-YYYY-number` or `GHSA-xxxx-xxxx-xxxx` identifier | `https://api.osv.dev/v1/vulns/{id}` | Advisory ID, aliases, modification/withdrawal dates, bounded package identities and fixed-version events |
| `password_prefix` | `prefix`, **exactly five hexadecimal SHA-1 characters** | `https://api.pwnedpasswords.com/range/{prefix}` with `Add-Padding: true` | Validated SHA-1 suffixes and occurrence counts; padding entries with zero counts are removed |
| `artifact_reputation` | `sha256`, **exactly 64 hexadecimal characters** | `https://www.virustotal.com/api/v3/files/{sha256}` with server-side `x-apikey` | Known analysis-category counts and analysis/submission dates only |

The OSV lookup deliberately does not assume that every CVE is an OSV record ID.
Use a GHSA or OSV identifier from a local scanner/advisory when available.
Ecosystem-specific advisory identifiers outside these two forms are currently
unsupported. The OSV summary omits descriptions, source excerpts and arbitrary
reference URLs. Its fixed-version list is not a dependency resolver: review the
full advisory's ecosystem ranges and project constraints before upgrading.

The service does not expose URL reputation, file submission, file downloading,
rescanning, arbitrary HTTP headers, arbitrary providers, remote commands, shell
execution, paths, source code or binary data as tool arguments. Unknown tool
names and extra fields are rejected rather than forwarded.

## Privacy and limits of the evidence

**Password prefixes.** Compute the SHA-1 hash in a trusted local application and
send only its first five hexadecimal characters. Keep both the password and
complete hash out of the model context, logs and tool arguments. Compare the
remaining 35 characters with the returned suffixes locally. The tool itself
cannot determine a match because it never receives the remaining hash. Do not
make a new query for each character typed. Padding reduces the response-size
signal on the provider connection; the prefix, source IP and query timing are
still visible to the provider. This is not credential validation against a live
account. A missing match does not prove a password is safe, and a corpus match
does not prove any particular account was breached. See the official
[HIBP range and padding documentation](https://haveibeenpwned.com/API/v3#PwnedPasswords).

**Artifact hashes.** VirusTotal receives a SHA-256 hash and the service account's
API identity, not the artifact itself. A hash can still reveal interest in a
particular artifact and can correlate with an existing report; submit only
identifiers the operator is authorized to disclose. HTTP 404 produces
`status: not_found` and `assessment: unknown`. Zero vendor detections do not
establish safety. The service returns neither filenames, engine detail strings,
raw provider JSON, nor downloaded samples. See the official
[file-report endpoint](https://docs.virustotal.com/reference/file-info) and
[file-object fields](https://docs.virustotal.com/reference/files).

**Vulnerability context.** CISA receives a catalog request, rather than the
specific CVE; FIRST receives the CVE identifier; OSV receives the advisory ID.
These services do not receive a source tree or dependency manifest. Public
exploitation evidence and population-level probabilities must be combined with
confirmed affected versions, exposure, reachability and impact. KEV absence or
a low EPSS score is not a clean bill of health. FIRST describes EPSS as a
probability over the next 30 days, with a distinct percentile ranking. See
[FIRST's usage guidance](https://www.first.org/epss/using-epss),
[its API](https://api.first.org/epss/),
[the official CISA data repository](https://github.com/cisagov/kev-data), and
[OSV's ID lookup](https://google.github.io/osv.dev/get-v1-vulns/).

**Email/account breach checking is not implemented.** There is no email, domain,
username, stealer-log or plaintext-secret input. Operators who need account
exposure monitoring should configure the official HIBP service separately,
establish the organizational authorization and data-retention policy, and verify
control of the domain using the provider's supported process. An API subscription
by itself does not authorize arbitrary searches about other people. See
[HIBP domain verification and search](https://haveibeenpwned.com/API/v3#DomainSearch).
No finding from this bridge is an online confirmation of a leaked working
credential or a promise that an account is uncompromised.

## Results, staleness and failure handling

Tool results have a compact JSON document in both MCP `structuredContent` and a
text content block. Successful provider lookups are `status: ok`; unknown records
are `not_found`. Errors are `error` with a fixed code and safe message. CVE
enrichment can be `partial` when either source fails, is absent or is stale.
OSV summaries are also partial if package/range/event limits truncate them.
An OSV withdrawal is explicit and must be considered before acting on that
advisory. Its modification date is not treated as a freshness expiry: a valid
old advisory may remain relevant.

Freshness thresholds are conservative local policy, not provider guarantees:

- KEV catalog release more than **30 days** old: `stale` source.
- EPSS score date more than **3 days** old: `stale` source.
- VirusTotal analysis more than **30 days** old: `stale`, with its age and dates.

Stale and partial results carry `isError: true`; available evidence is retained.
Neither source failures nor missing API keys become empty successful findings.
No source's raw error body, redirect URL or exception text is returned. Static
error codes distinguish missing configuration, denied provider access, rate
limits, malformed responses, redirects, response-size limits and timeouts.
The bridge never triggers a provider rescan to refresh old intelligence.

The service retains no provider response cache or on-disk query history and does
not log request identifiers, prefixes or credentials. Normalized results may
still be retained by the MCP client and Boxoro's conversation history; operators
must account for that retention. Upstream providers have their own policies.

## Resource bounds

| Boundary | Enforced bound |
| --- | --- |
| Request line / aggregate headers | 2 KiB / 16 KiB; oversized requests rejected |
| JSON request body | 8 KiB; exactly one Content-Length required; no chunked or compressed requests |
| Active connections | 4 by default, configurable from 1 to 8; excess receives HTTP 503 |
| Total connection lifetime | 30 seconds, including slow request bodies |
| Provider worker | At most 25 seconds, further limited by the connection's remaining budget; entire child process group killed on expiry |
| Individual upstream socket operations | 8-second timeout within the hard worker deadline |
| Upstream body | KEV 8 MiB; OSV and VirusTotal 512 KiB; password range 128 KiB; EPSS 64 KiB |
| Password range rows | 2,048; over-limit or malformed ranges fail rather than silently truncate |
| Normalized worker result / MCP response | 240 KiB / 512 KiB |
| Example systemd service | 512 MiB memory, 200% CPU quota, 64 tasks, 256 file descriptors |

These limits are not a quota manager for paid services. The service performs no
automatic retries and does not hide upstream 429 responses. Set appropriate
permitted workflows and client-side call budgets for the provider subscription.
Repeated CVE queries currently refetch the bounded KEV catalog; this bridge is
for occasional enrichment, not bulk intelligence ingestion.

## Configuration and optional disabling

| Environment variable | Default | Behavior |
| --- | --- | --- |
| `SECURITY_INTEL_TOKEN` | Required | 32–256 visible ASCII characters; generate a cryptographically random token, not a repeated or human-chosen string |
| `SECURITY_INTEL_PORT` | `8767` | Port 1–65535; bind address remains `127.0.0.1` |
| `SECURITY_INTEL_MAX_CONCURRENCY` | `4` | Integer from 1 to 8 |
| `SECURITY_INTEL_TOOLS` | All four tools | Comma-separated enabled tool names; an empty value disables every tool; unknown or duplicate names fail startup |
| `VT_API_KEY` | Unset | Required only for reputation; with no key, that tool returns `not_configured` without a network request |

To enable vulnerability context only, set:

```ini
SECURITY_INTEL_TOOLS=cve_context,osv_vulnerability
```

Also narrow the MCP entry's `allowed_tools`. The service policy enforces disabling
even if a caller bypasses tool discovery. Removing this server's entry from the
operator MCP file disables the integration entirely; local scanning still works.

Free/no-key services: CISA KEV, FIRST EPSS, OSV and HIBP Pwned Passwords. HIBP's
password API does not require a breach-search subscription or API key. Do not
confuse it with the authenticated account/domain APIs.

**VirusTotal use must fit the chosen license.** Its published Public API terms
limit it to 4 requests/minute and 500/day, prohibit use in commercial products or
services, and prohibit business workflows that do not contribute new files.
This bridge intentionally performs hash-only lookups and never uploads files.
For business use, obtain an appropriate licensed service agreement or disable
`artifact_reputation`; do not work around the terms by adding upload behavior.
Premium pricing and quotas depend on the agreement. Confirm the current
[Public vs Premium API terms](https://docs.virustotal.com/reference/public-vs-premium-api)
before enabling a key. A configured key is not evidence of a suitable license.

## Installation on the control machine

This repository supplies an example, not a deployed service. Use an operator
account with host-administration authorization. Python 3.10 or later and its
standard library are sufficient; no pip dependencies or scanner packages are
needed for this bridge.

1. Create the dedicated service user `boxoro-security-intel` with no login shell
   or home directory. Install the Python file as a root-owned, mode-0644 file at
   `/opt/agent-rush/security/intel_server.py` in a root-owned directory. Install
   the unit at `/etc/systemd/system/security-intel.service`.
2. Create `/etc/agent-rush/security-intel.env` as **root-owned mode 0600**. Set a
   freshly generated `SECURITY_INTEL_TOKEN`, the desired enabled tools, and
   optional provider key. Provision secrets through your existing secret manager
   or protected host editor. Never paste real values into a prompt or checked-in
   file. systemd reads the environment file before changing to the service user.
3. Copy the JSON example to an operator-only file, for example
   `/etc/agent-rush/security-mcp-servers.json`. Replace the placeholder with
   `Bearer ` followed by the exact same token. Make the resulting file mode 0600,
   owned by the account running Boxoro's control plane (`agent-rush` in the
   supplied control unit). Do not give the intelligence service user access to
   the control plane's other credentials.
4. Set `AR_MCP_SERVERS_FILE` in the control plane's protected environment file.
   If it already names another MCP configuration, **merge the new server entry
   into that configuration** and preserve existing servers instead of replacing
   them. Ensure `allowed_tools` matches the desired policy. Restart the control
   plane using the normal operator rollout procedure after reviewing the change.
5. Permit the fixed provider HTTPS hosts and DNS for this service through the
   control machine's egress policy. No worker firewall rule or workspace secret
   is needed. Environment HTTP proxies are not used by this bridge. A provider
   block produces an explicit unavailable result; do not disable TLS checks or
   broaden arbitrary URL access to work around it.
6. Run `systemctl daemon-reload`, then `systemctl enable --now security-intel`.
   The unit uses a dedicated user, read-only filesystem protection, no Linux
   capabilities, no home access, and resource limits. Inspect `systemctl status
   security-intel` for startup failures without copying secrets into tickets.

After operator registration, enable `security-intel` in the agent configuration's
`mcp_servers` list using Boxoro's existing `agent_config` workflow, preserving any
other enabled servers. Ensure `mcp` is not in `disabled_tools`. Operator-available
servers and agent-enabled servers are separate settings. Example tool calls once
enabled:

```json
{"tool":"mcp","action":"list_tools","server":"security-intel"}
```

```json
{"tool":"mcp","action":"call","server":"security-intel","name":"cve_context","arguments":{"cve":"CVE-2021-44228"}}
```

You can generate a token directly into an **exclusive new file** without printing
it, for subsequent provisioning by your secret manager:

```bash
python3 - <<'PY'
import os
import secrets
fd = os.open('security-intel.env', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
with os.fdopen(fd, 'w') as output:
    output.write('SECURITY_INTEL_TOKEN=' + secrets.token_hex(32) + '\n')
    output.write('SECURITY_INTEL_TOOLS=cve_context,osv_vulnerability\n')
PY
```

The file contains a real secret: keep it outside the repository and transport it
using the operator's secure provisioning process. The example deliberately
starts with only vulnerability context enabled. For containerized control
deployments, do not place this environment file in a workspace bind mount.

## Verification and operating checks

Run the offline behavior suite from the repository root:

```bash
python3 -m unittest discover -s qa -p test_security_intel.py -v
```

Tests exercise a real loopback listener, the Boxoro initialization sequence,
authentication/origin/Host enforcement, strict schemas, body and result limits,
concurrency, slow-body closure, fixed upstream requests, redirect handling,
provider-error normalization, secret redaction and child-process termination.
External HTTP boundaries use deterministic fixtures; tests need no provider
accounts, actual passwords, uploaded artifacts or live API keys.

For an operator smoke check, use Boxoro's existing `mcp` tool to list the
`security-intel` server and request one documented public CVE. Inspect both KEV
and EPSS statuses and dates. If reputation is enabled without a key, confirm that
it reports `not_configured`. A live provider request proves connectivity only at
that time. The automated suite does not certify provider availability, an
account's quota or license, a production systemd installation, or the absence of
malware and compromise.
