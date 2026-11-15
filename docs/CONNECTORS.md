# Specialist connectors and external services

Local specialist tools execute inside the existing Boxoro worker. Account-backed services use Boxoro's existing control-side Streamable HTTP MCP client. Client headers and credentials remain on the control side. The catalog records ten maintained server families or implementation references, and each specialist guide states which ones can help its workflows.

## Register a concrete HTTP service

1. Choose a specific maintained server/version and obtain access to the account or self-hosted service that the user wants to use.
2. Follow that vendor's deployment and authentication instructions. For a local service, bind to loopback or a restricted control network; require authentication even if a port is not public.
3. Merge a server record into the protected file named by `AR_MCP_SERVERS_FILE`. Preserve existing entries. Use a descriptive name, the actual HTTP MCP URL, authentication headers and an explicit `allowed_tools` array.
4. Reload/restart the control service according to its existing deployment process. Enable that registered server in the owner's `mcp_servers` configuration, preserving other config fields.
5. Use the existing native `mcp` action `list` to verify the server's real exposed tool names, then call only the intended tools. An empty `allowed_tools` array deliberately exposes no tools. Omitting that property allows the server's tools, so do not omit it accidentally.
6. Test with a harmless read, confirm account/project scope and record missing prerequisites. A configured URL is not proof that the account has access or that a provider API key works.

`deploy/agents/mcp-servers.example.json` is a concrete starting shape for official read-only GitHub and the included OpenBB service. Replace placeholder bearer values in a private copy; the config loader does not expand `${ENV_VAR}` inside JSON. Never commit populated credentials. The two services are independent; remove entries that you do not deploy.

The operator-configured remote endpoint remains a trusted integration. Tool-name allowlisting does not inspect every argument, enforce database read-only transactions, limit cloud IAM or authorize a message to another person. Configure those controls at the account/service layer and follow the user's task authorization.

## Verified integration map

| Connector | Practical Boxoro integration | Credentials / prerequisites | Default boundary |
| --- | --- | --- | --- |
| [GitHub official MCP](https://github.com/github/github-mcp-server) | Remote `https://api.githubcopilot.com/mcp/`; register in `AR_MCP_SERVERS_FILE`. | Authorized scoped PAT or supported service-managed OAuth; intended repository access. | Request `X-MCP-Readonly: true` and selected toolsets, then allow specific read tools. Opening PRs, commenting, merging and releasing require task authorization and a separate suitable write configuration. |
| [Playwright MCP](https://github.com/microsoft/playwright-mcp) | Optional separately deployed official server. Local browser testing already works through Boxoro's native browser and registered Playwright CLI. | A pinned server package, compatible browser and disposable profile; a supported reviewed HTTP mode if connecting from Boxoro. | Keep page access scoped and profiles disposable. Installing the Playwright test CLI does not install or connect this MCP server. |
| [Chrome DevTools MCP](https://github.com/ChromeDevTools/chrome-devtools-mcp) | Optional specialized performance/debugging integration. | Reviewed server, compatible Chrome and explicitly configured debugging endpoint. A stdio-only setup cannot be pasted into Boxoro's HTTP configuration. | Debugging can expose page data and execute interactions; do not reuse personal browser profiles implicitly. |
| [Grafana MCP](https://github.com/grafana/mcp-grafana) | Operator-deployed vendor HTTP service for approved observability sources. | Explicit Grafana URL, limited service-account token and separately restricted datasource identities. | Use the vendor's `--disable-write` mode and selected tool categories. A read-only MCP mode does not make arbitrary datasource SQL read-only. |
| [Sentry MCP](https://mcp.sentry.dev/) | Vendor HTTP endpoint; use a supported authenticated deployment/account integration. | Organization/project authorization and current vendor-supported auth. | Prefer project-scoped endpoints and reviewed issue/performance reads. Boxoro's header client is not an interactive OAuth enrollment UI; complete vendor account setup separately. |
| [AWS official tools](https://github.com/awslabs/mcp) | Select a named official server or the maintained Agent Toolkit for AWS successor for the required service. | Approved AWS account/profile, concrete server deployment and narrow IAM. | Documentation and diagnostics first. No universal AWS MCP endpoint or automatically provisioned cloud account is assumed. |
| [MCP Toolbox for Databases](https://github.com/googleapis/mcp-toolbox) | Operator-deployed HTTP server with reviewed parameterized query tools. | Explicit database/server, read-only database role and relevant IAM/auth configuration. | Avoid unrestricted `execute_sql` against production. Server flags do not replace SQL grants and database policies. |
| [Brave Search MCP](https://github.com/brave/brave-search-mcp-server) | Official package with a separately reviewed HTTP deployment when needed. Boxoro also has its existing web-search helper. | Brave API key and plan entitlement. | Scope searches; treat snippets as leads and verify consequential claims at primary sources. |
| [MCP SDK/reference servers](https://github.com/modelcontextprotocol/servers) | Development resources for Agent Foundry; select an actual maintained implementation before deployment. | Reviewed roots, resources, package pin and HTTP interface. | Reference examples require hardening. Do not use archived GitHub/Postgres/SQLite reference servers as maintained production integrations. |
| [Boxoro Finance OpenBB](FINANCE_AGENT.md) | Implemented authenticated local HTTP service at `http://127.0.0.1:8768/mcp`; build the separate `finance-service` image. | Private service token, trusted private state, optional source-specific provider keys and data entitlements. | Four fixed tools expose discovered read/analytics schemas. No generic shell, install operation, account-settings route or order execution. |

These records are not active connections. Deployment is intentionally not inferred from vendor README example URLs. External accounts, OAuth grants, API subscriptions, devices, clusters, data licenses and production database identities are not created by a profile selection.

## Protocol compatibility

Boxoro currently implements its existing MCP `2025-06-18` compatibility level. The Finance adapter speaks the same version. A vendor using a newer MCP draft must still support compatible initialization, HTTP requests and responses. This implementation does not claim support for every newer protocol feature, interactive OAuth flow, stdio transport, SSE behavior or vendor-specific extension.

For any future bridge from stdio to HTTP, prefer a maintained vendor-supported server mode with authentication, process limits and an explicit tool set. Do not add an unrestricted control-host process launcher as a workaround. The native CLI operations already cover the default local specialist workflows without that extra trust boundary.

## Finance deployment

The complete provider, token, state ownership, Compose, MCP and test instructions are in [FINANCE_AGENT.md](FINANCE_AGENT.md). The committed [provider register](OPENBB_PROVIDERS.md) reflects the installed OpenBB V5 packages; removed V4 providers are not silently advertised as available.

## Sources and updates

The canonical connector records and source URLs are in `deploy/agents/catalog.json`. The tool [research register](AGENT_TOOL_RESEARCH.md) includes licensing, image/optional/host distinctions and package choices. Recheck vendor transport/auth requirements when upgrading a server; do not reuse an old tool allowlist without inspecting its current schemas.
