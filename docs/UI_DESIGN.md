# Workspace visual system

Concept: /workspace/generated_images/exec-1a96790f-a803-4167-85fd-008bdf0d7716.png. This is a visual reference, never a replacement for live controls or evidence.

Use true white #ffffff, cool sidebar #eef2f5, dark ink #0b142c, secondary #596277, pine #00613c, soft pine #dfebe5, borders #d5dbe2 and orange #b84b16. System sans-serif, title 44px/1.12 bold, section 22px, control 14px, mono logs 13px. Sidebar 220px, main max width1240 with 28px gutters, 8px corner radii, fine borders, no shadows or gradients.

The primary screen has sidebar navigation, slim breadcrumb/login header, headline and subtitle, two-column task form/boundary explanation, recent-run table/empty state, and three restrained footer labels. Mobile moves navigation into a horizontal compact rail and stacks form/boundary while maintaining real controls and readable logs.

Allowed main copy: Agent Rush; Workspace; Run history; Evidence; Safety & limits; Local workspace; Operator login; Give every agent a world.; Real execution. Clear boundaries. Proof of cleanup.; What should the agent do?; CLI sandbox; Network disabled; Memory; Timeout; Run in sandbox; Execution boundary; Rust controller; Policy broker; Disposable gVisor sandbox; Recent runs; TASK; STATE; DURATION; CLEANUP; Your first run starts here.; Executed results and cleanup evidence will appear after a task runs.; Isolated by default; Bounded resources; Evidence you can export.

Required intentional functional extensions: login dialog, actual configuration/worker status, task detail with events/logs/evidence download and cancel, safety dialog, errors, and explicit unavailable-profile messaging. The concept's 'No cloud machines running' is only shown when no deployment has been made by this project; never infer account-wide machine status from local configuration. Evidence/export controls act on real persisted data only. Memory/timeout/network selectors show fixed supported policy, not fictitious optional capabilities.
