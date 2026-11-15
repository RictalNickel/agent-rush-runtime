# Specialist verification and efficiency follow-up

## Intent

Verify that the implemented tools fit their assigned tasks, improve useful workflows, and reduce repeated work. The user authorized implementation and GitHub publication in the existing specialist branch. This is a follow-up to the delivered system, not a new deployment or a request to provision external accounts.

## Evidence and implementation decisions

The current native helper describes an entire profile (approximately 14–28 KB) and probes every profile tool even when the user wants one operation. Installed-tool fixtures also exposed incorrect recipe coverage and exit classifications. Finance imports and expands every OpenBB schema for metadata and narrow query calls. Image installation has avoidable cache/layer work that can be examined without claiming a Docker build.

Three approaches were considered: add more dependencies, run all probes concurrently, or reduce work to the selected task and correct existing recipes. The implementation decision is the third approach. It uses already installed tools, preserves serial process cleanup and isolation, and produces measurable reductions without introducing a new scheduler or sharing provider query state. Download caching is limited to verified build inputs; financial results are not cached.

## Changes

- Allow optional, mutually exclusive operation/workflow selectors for `describe` and `doctor`; validate profile membership and show the exact scope and required effects.
- Preserve full-profile inspection for image audits. Scoped success applies only to selected capabilities.
- Correct tool arguments, effect declarations, findings/error distinctions, and zero-input coverage. Add useful fixed operations only after real CLI fixtures establish their behavior.
- Reduce repeated OpenBB schema work with trusted service-lifetime metadata while retaining fresh isolated query workers and bounded requests.
- Improve installer correctness and reusable download/layer work while preserving hash verification, lifecycle-script restrictions, and independent environments.
- Document task-oriented usage, actual before/after measurements, and unexecuted deployment checks.

## Verification and release boundary

Use regression fixtures for incorrect behavior, actual installed tools for recipe checks, and measured before/after runs for efficiency claims. Run the combined Python and Rust checks after integration. No Docker/BuildKit daemon, live provider keys, mobile devices, or remote accounts are present; their qualification remains outstanding. Publish the verified follow-up to PR #3 without merging or deploying.
