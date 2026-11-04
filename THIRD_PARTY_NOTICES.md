# Third-party notices

Agent Zero, copyright 2025 Agent Zero, s.r.o, MIT license. Source reference: https://github.com/agent0ai/agent-zero at ba9b67df0f54ac78106ae85c552ef5d2458f7c9a. Its context hierarchy, bounded agent loop and tool lifecycle informed this native Rust implementation. Agent Rush is not a GitHub fork, does not embed the Python runtime, and does not claim full Agent Zero compatibility. License preserved in docs/reference/AGENT_ZERO_LICENSE.

Rust dependencies are pinned by Cargo.lock and their source/license notices accompany the downloaded Cargo registry packages. Axum, Tokio, serde, reqwest, rusqlite, rustls and the other crates retain their upstream licenses. Do not represent these libraries as rewritten components.

Execution uses the official rust:1.90.0-bookworm OCI image pinned to sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f. Rust and Debian packages retain their respective licenses. Docker and gVisor are infrastructure dependencies, not ports authored by this project. gVisor is Apache-2.0; Docker licensing depends on the distributed component. No Docker Desktop dependency is required.

Playwright is an Apache-2.0 development/test dependency, not a production server dependency. Browser-native HTML/CSS/JavaScript assets support the Rust web server.
