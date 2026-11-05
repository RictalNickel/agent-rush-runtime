# Third-party notices

Rust dependencies are pinned by Cargo.lock and their source/license notices accompany the downloaded Cargo registry packages. Axum, Tokio, serde, reqwest, rusqlite, rustls and the other crates retain their upstream licenses. Do not represent these libraries as rewritten components.

Execution uses the official rust:1.90.0-bookworm OCI image pinned to sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f. Rust and Debian packages retain their respective licenses. Docker and gVisor are infrastructure dependencies, not ports authored by this project. gVisor is Apache-2.0; Docker licensing depends on the distributed component. No Docker Desktop dependency is required.

Playwright is an Apache-2.0 development/test dependency, not a production server dependency. Browser-native HTML/CSS/JavaScript assets support the Rust web server.

Vultr govultr SDK, MIT license, copyright 2019 Vultr, inspected at ab4d9052f7bba410fd63a3f9129e06bd5e14aae1 as an API endpoint/schema reference only. No Go runtime dependency or fork is introduced. License preserved in docs/reference/VULTR_SDK_LICENSE.

Archivo variable font (latin subset, weight and width axes), copyright 2020 The Archivo Project Authors (https://github.com/Omnibus-Type/Archivo), SIL Open Font License 1.1. Taken unmodified from the @fontsource-variable/archivo 5.3.0 package and served from `/assets/archivo.woff2`, because the content security policy allows no third-party origins. License preserved in web/fonts/OFL.txt.
