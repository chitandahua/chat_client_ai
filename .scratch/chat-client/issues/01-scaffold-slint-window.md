# 01 — Scaffold + Slint window boots

**What to build:** A Rust crate using Slint 1.17.1 that runs on Linux: `cargo run` opens a window showing the login screen (server host:port, username, password fields, a login button) with no network attached. Establishes the project skeleton (build.rs compiling the `.slint` markup), the Slint event loop, and the UI→Rust callback wiring (`do-login` etc. wired but a no-op or status text for now).

**Blocked by:** None — can start immediately.

**Status:** ready-for-agent

- [ ] `cargo run` opens a Slint window titled for the chat app
- [ ] Login screen renders (server host, username, password fields + login button)
- [ ] Pressing login sets a status message ("connecting…") but performs no network call
- [ ] Builds cleanly (cargo build) with slint + slint-build pinned per spec versions
