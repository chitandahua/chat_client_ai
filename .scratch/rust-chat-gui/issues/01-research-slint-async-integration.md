# Slint async event-loop integration patterns

Blocked by:
Type: research
Status: resolved

## Question

How should the Rust chat client structure a Slint (1.x) desktop app so an async network stack (tokio) drives a live TCP connection and pushes events into the Slint UI thread? What are the current idiomatic mechanisms — `slint::spawn_local`, `invoke_from_event_loop`, `Timer`, component-scoped callbacks, `#[callback]` — and what does a minimal skeleton (Cargo.toml, a `.slint` file, and the event bridge) look like? Capture current crate versions and any breaking API notes for Slint 1.x.

## Answer

Full findings: `.scratch/rust-chat-gui/research/01-slint-async-integration.md` (verified against Slint 1.17.1, 2026-08-08).

- **Versions**: slint + slint-build `1.17.1` (MSRV 1.92), tokio `1.53.1`, reqwest `0.13.4`, serde `1.0.229`, serde_json `1.0.151`, async-compat `0.2.5`. Winit is the default backend on all platforms since 1.16.
- **No `AsyncComponentRunner`, no `#[callback]`** — both are invented/outdated names. Callbacks are declared in `.slint` (`callback send-message(string);`) and wired from Rust via generated `.on_send_message(...)`.
- **Recommended architecture for live TCP + tokio**: main thread runs `ui.run()`; spawn a multi-threaded tokio runtime on a background thread owning the socket; network→UI via `Weak::upgrade_in_event_loop(|ui| ...)` (or `slint::invoke_from_event_loop`); UI→network via `.slint` callbacks forwarding into a `tokio::sync::mpsc` channel (`blocking_send`). Short-lived ops (e.g. HTTP login) can stay on the UI thread with `slint::spawn_local(async_compat::Compat::new(fut))`.
- **Pitfalls**: don't block the event loop; `Timer`/`spawn_local` are UI-thread-only; tokio current-thread scheduler can't run on the Slint main thread; `#[tokio::main]` discouraged (wrap `run_event_loop` in `block_in_place`); capture `Weak`, never strong handles, in callback closures.
