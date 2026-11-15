# Specialist operating guides

Each guide documents the embedded operating skill, tool packages, fixed operations, workflows, dependencies and limits. Selecting a specialist changes guidance; it does not change the worker sandbox or grant an external account.

| Specialist | Family | Tools | Workflows |
| --- | --- | ---: | --- |
| [Agent Foundry](agent-foundry.md) | `python` | 15 | `preflight`, `test`, `scaffold` |
| [TestForge](testforge.md) | `python` | 15 | `python-gate`, `browser-gate`, `api-contract` |
| [API Architect](api-architect.md) | `browser` | 14 | `lint-contract`, `breaking-change`, `protobuf` |
| [Performance Lab](performance-lab.md) | `python` | 13 | `http-baseline`, `python-profile`, `memory-report` |
| [ReleasePilot](releasepilot.md) | `platform` | 15 | `source-release-check`, `container-check`, `workflow-check` |
| [CloudPilot](cloudpilot.md) | `platform` | 15 | `infra-preflight`, `kubernetes-check`, `chart-check`, `checkov-qualified` |
| [OpsWatch](opswatch.md) | `platform` | 13 | `alert-rules`, `config-check`, `incident-evidence` |
| [Database Pro](database-pro.md) | `data` | 14 | `sql-quality`, `sqlite-schema`, `duckdb-schema`, `backup-index` |
| [Accessibility](accessibility.md) | `browser` | 12 | `page-audit`, `markup`, `browser-regressions`, `axe-qualified` |
| [AI Eval Lab](ai-eval-lab.md) | `python` | 14 | `retrieval-metrics`, `model-eval`, `eval-tests` |
| [Code Architect](code-architect.md) | `python` | 15 | `python-architecture`, `dependency-check`, `duplication` |
| [DataWorks](dataworks.md) | `data` | 14 | `csv-quality`, `sql-quality`, `json-contract` |
| [DocSmith](docsmith.md) | `documents` | 15 | `markdown-quality`, `build-mkdocs`, `pdf-check`, `prose-style` |
| [VisualQA](visualqa.md) | `browser` | 12 | `capture`, `compare`, `regression-suite`, `odiff-baseline` |
| [PrivacyGuard](privacyguard.md) | `python` | 15 | `local-review`, `dependency-audit`, `static-security` |
| [LocaleQA](localeqa.md) | `browser` | 14 | `json-parity`, `gettext-quality`, `icu-catalog`, `translation-metrics` |
| [RestoreOps](restoreops.md) | `platform` | 13 | `archive-review`, `compare-restore`, `restic-structure` |
| [SiteHealth](sitehealth.md) | `browser` | 14 | `page-health`, `links`, `documentation-links` |
| [MobileLab](mobilelab.md) | `mobile` | 15 | `apk-static`, `source-audit`, `mobile-web` |
| [Knowledge Auditor](knowledge-auditor.md) | `documents` | 15 | `corpus-integrity`, `pdf-evidence`, `local-html`, `ocr-evidence` |
| [Finance Lab](finance-lab.md) | `finance` | 1 | `readiness` |

See [runtime](../SPECIALIST_RUNTIME.md), [research and source register](../AGENT_TOOL_RESEARCH.md), and [Finance Lab architecture](../FINANCE_AGENT.md).
