# Slint async event-loop integration patterns (Rust)

Research for ticket "Slint async event-loop integration patterns". All API facts verified against Slint 1.17.1 (latest stable, 2026-07-07) docs.rs / docs.slint.dev / the slint-ui/slint repo.

## Answer

- **Current Slint version: `1.17.1`** (crates: `slint`, `slint-build` both `1.17.1`; MSRV is Rust 1.92 since 1.17.0).
- **`AsyncComponentRunner` does NOT exist** — verified absent from the slint crate's full public item list and from the whole slint-ui/slint repo. Do not use it; there is no such API.
- There is **no `#[callback]` Rust attribute** either. Callbacks are declared in the `.slint` file (`callback foo(string) -> string;`) and wired from Rust with the generated `.on_foo(...)` / `.invoke_foo(...)` methods. That is the "component-scoped callback" mechanism.
- **The three official bridging primitives** (all in crate `slint`, all usable with tokio in the same binary):
  1. `slint::spawn_local(fut) -> Result<JoinHandle<F::Output>, EventLoopError>` — run an async future *on the Slint event loop*, **UI thread only**. Tokio-backed futures **must be wrapped in `async_compat::Compat::new(...)`** (async-compat then owns/allocates a shared multi-threaded tokio runtime). This is the pattern of Slint's official `examples/async-io` stock ticker.
  2. `slint::invoke_from_event_loop(|| { ... }) -> Result<(), EventLoopError>` — queue a `FnOnce + Send` closure to run **on the UI thread; callable from any thread** (your tokio/network threads).
  3. `Weak<T>::upgrade_in_event_loop(|handle| { ... }) -> Result<(), EventLoopError>` — the recommended convenience: upgrade a component weak-handle and touch Slint state, all safely marshalled onto the UI thread from a background tokio task.
- `slint::Timer` (with `TimerMode::{Repeated, SingleShot}`) exists for delays/intervals, but **only fires on the UI thread**. It is for periodic UI ticks / polling, not the primary network bridge.
- **Recommended architecture for a live TCP connection driven by tokio:**
  - Main thread: create component, `ui.run()` (runs the winit event loop).
  - Background: `std::thread::spawn` a `tokio::runtime::Builder::new_multi_thread()` runtime that owns the socket/connection task.
  - Network → UI: from the tokio task, call `ui_weak.upgrade_in_event_loop(move |ui| { ui.set_...(data) })` (or `slint::invoke_from_event_loop` + `Weak::upgrade`).
  - UI → network: declare `callback`(s) in `.slint`, attach `ui.on_send_message(...)` etc., forward into the tokio task via a `tokio::sync::mpsc` channel.
  - Short-lived async ops (e.g. a one-off HTTP GET) can instead stay on the UI thread with `slint::spawn_local(async_compat::Compat::new(fut))` — see Details.
- **Tokio coexists fine with Slint.** Constraints: (a) don't block the event loop — do heavy work off-thread or animations/render freeze; (b) `Timer`/`spawn_local` only from the UI thread; (c) tokio's current-thread scheduler cannot run on the Slint main thread; (d) `#[tokio::main]` is **not recommended** — if you must, wrap the call to `slint::run_event_loop()` in `tokio::task::block_in_place`.
- **Relevant 1.x notes:** 1.17.1 fixed a panic when a future's waker fires after the event loop stopped; 1.16.0 made winit the default backend on all platforms (Qt no longer default on Linux); `invoke_from_event_loop`/`quit_event_loop` have returned `Result` since 1.0.0 (unwrap them).
- **Current dep versions (verified crates.io 2026-08-08):** slint `1.17.1`, slint-build `1.17.1`, tokio `1.53.1`, reqwest `0.13.4`, serde `1.0.229`, serde_json `1.0.151`, async-compat `0.2.5`.

## Details

### Mechanism 1 (recommended for live TCP + tokio): background tokio runtime + `upgrade_in_event_loop`

`Cargo.toml`

```toml
[package]
name = "slint-tokio-chat"
version = "0.1.0"
edition = "2021"
build = "build.rs"

[dependencies]
slint = "1.17.1"
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "net", "io-util", "sync", "time"] }
reqwest = { version = "0.13.4", features = ["json"] }   # HTTP; drop if TCP-only
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
async-compat = "0.2.5"   # only needed for Mechanism 2 (spawn_local + tokio futures)

[build-dependencies]
slint-build = "1.17.1"
```

`build.rs`

```rust
fn main() {
    slint_build::compile("ui/app.slint").unwrap();
}
```

`ui/app.slint` (tiny markup)

```slint
import { Button, VerticalBox, HorizontalBox, ScrollView } from "std-widgets.slint";

export component ChatWindow inherits Window {
    title: "Slint + tokio chat";
    width: 480px;
    height: 640px;

    in-out property <string> connection-status: "disconnected";
    in-out property <[string]> messages: [];

    callback connect();
    callback disconnect();
    callback send-message(string text);

    VerticalBox {
        Text { text: root.connection-status; }
        ScrollView {
            vertical-stretch: 1;
            for msg in root.messages: Text { text: msg; }
        }
        HorizontalBox {
            Button { text: "Connect"; clicked => { root.connect(); } }
            Button { text: "Disconnect"; clicked => { root.disconnect(); } }
        }
        HorizontalBox {
            Button { text: "Send"; clicked => { root.send-message(input.text); } }
            LineEdit { text: ""; }
        }
    }
}
```

`main.rs` — the glue

```rust
slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = ChatWindow::new()?;

    // UI -> network: mpsc channel into the tokio task.
    let (ui_to_net, mut net_rx) = tokio::sync::mpsc::channel::<String>(64);
    let ui_handle = ui.as_weak();

    ui.on_connect({
        let tx = ui_to_net.clone();
        move || { let _ = tx.blocking_send("connect".into()); }
    });
    ui.on_disconnect({
        let tx = ui_to_net.clone();
        move || { let _ = tx.blocking_send("disconnect".into()); }
    });
    ui.on_send_message(move |text: slint::SharedString| {
        let _ = ui_to_net.blocking_send(text.to_string());
    });

    // Network -> UI: own tokio runtime on a background thread; it owns the live TCP connection.
    std::thread::Builder::new().name("net".into()).spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        rt.block_on(async {
            // Long-lived task: read commands from the UI and drive the socket.
            tokio::spawn(async move {
                while let Some(cmd) = net_rx.recv().await {
                    match cmd.as_str() {
                        "connect" => { /* connect socket, spawn reader loop */ }
                        "disconnect" => { /* drop socket */ }
                        text => { /* write text to socket */ }
                    }
                }
            });

            // Every inbound message is marshalled onto the UI thread:
            let weak = ui_handle;
            let _ = weak.upgrade_in_event_loop(move |ui: ChatWindow| {
                ui.set_connection_status("connected".into());
                let mut rows: Vec<slint::SharedString> = ui.get_messages().iter().collect();
                rows.push(slint::SharedString::from("peer: hello"));
                ui.set_messages(rows.into());
            });
        });
    })
    .expect("spawn net thread");

    ui.run()
}
```

Notes:
- `Weak<ChatWindow>` is `Send` (docs: `impl Send for Weak<T>`), so it can cross into the tokio thread. `upgrade_in_event_loop` upgrades it only on the UI thread and drops the closure if the component is gone.
- Generated name mapping: `connection-status` → `set_connection_status`/`get_connection_status`; `send-message` → `on_send_message`; all dashes become underscores.
- Don't touch the component (setters/getters/Timer/VecModel) from the tokio thread directly — always go through `upgrade_in_event_loop`/`invoke_from_event_loop`.

### Mechanism 2 (official example): `slint::spawn_local` + `async_compat::Compat`

From Slint's own `examples/async-io` (README: "These are run inside a future run with `slint::spawn_local()`, where we can await for the result of the network request and update the UI directly - as we're being run in the UI thread"). Best for short-lived async operations initiated from the UI:

```rust
async fn fetch() -> String {
    let resp = reqwest::get("https://example.com/data.json").await.unwrap();
    resp.text().await.unwrap()
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = ChatWindow::new()?;
    ui.show()?;

    ui.on_connect(move || {
        let weak = ui.as_weak();
        // Compat::new allocates/enters a shared multi-threaded tokio runtime so reqwest works.
        slint::spawn_local(async_compat::Compat::new(async move {
            let body = fetch().await;
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_connection_status(body.into());
            });
        }))
        .unwrap();
    });

    ui.run()
}
```

The `slint::spawn_local` docs add: "Tokio futures … may not complete, because the Slint runtime can't drive the Tokio runtime … To address these constraints, use async_compat's `Compat::new()` … to implicitly allocate a shared, multi-threaded Tokio runtime."

### Mechanism 3 (cross-thread, synchronous-ish): `slint::invoke_from_event_loop`

```rust
// from any thread (e.g. a std::thread or tokio task):
let weak = ui_handle.clone();                 // ui_handle: slint::Weak<ChatWindow>
slint::invoke_from_event_loop(move || {
    if let Some(ui) = weak.upgrade() {
        ui.set_connection_status("connected".into());
    }
}).unwrap();
```

Same thing, one call: `weak.upgrade_in_event_loop(move |ui| { ... })`.

### Mechanism 4: `slint::Timer` (UI-thread periodic ticks)

```rust
use slint::{Timer, TimerMode};
let timer = Timer::default();
timer.start(TimerMode::Repeated, std::time::Duration::from_millis(1000), || {
    // only on the UI thread; e.g. poll a shared AtomicBool / channel for new data
});
// keep `timer` alive; stops automatically on drop.
```

### Pitfalls (verified from docs)

1. **Event loop must run on the main thread** in most backends; components must be created on the same thread that runs (or will run) the event loop. (crate-level "Threading and Event-loop" docs)
2. **Don't block the event loop** — "perform the minimum amount of work in the main thread and delegate the actual logic to another thread to avoid blocking animations."
3. **`spawn_local` is UI-thread-only**; from another thread, hand the future to the event loop via `invoke_from_event_loop` instead.
4. **Tokio current-thread scheduler cannot be used on the Slint main thread.** Prefer a multi-threaded runtime on a background thread, or `async_compat::Compat`.
5. **`#[tokio::main]` is not recommended**; if used, wrap the event loop entry in `tokio::task::block_in_place(slint::run_event_loop)`.
6. **`Timer` only fires on the event-loop thread**; timers started elsewhere never fire.
7. Strong handles in callback closures create reference loops → capture `Weak` and upgrade inside the handler.

## Sources

- https://docs.slint.dev/ — docs index (Slint 1.17.1 docs)
- https://docs.slint.dev/latest/docs/rust/slint/ — slint crate reference (threading/event-loop guidance, generated-component API, callback wiring)
- https://docs.slint.dev/latest/docs/slint/language-integrations/ — language integrations index (Rust API docs pointer)
- https://docs.rs/slint/latest/slint/fn.spawn_local.html — `spawn_local` signature + Tokio compatibility + async_compat guidance
- https://docs.rs/slint/latest/slint/fn.invoke_from_event_loop.html — `invoke_from_event_loop` signature + cross-thread example
- https://docs.rs/slint/latest/slint/struct.Weak.html — `Weak`/`upgrade_in_event_loop` (Send, upgrade only on creator thread)
- https://docs.rs/slint/latest/slint/struct.Timer.html — `Timer`, `TimerMode`, `Timer::single_shot`
- https://docs.rs/slint/latest/slint/ — full crate item index (confirming NO `AsyncComponentRunner`, NO `#[callback]`)
- https://github.com/slint-ui/slint/blob/master/examples/async-io/main.rs and README.md and Cargo.toml and stockticker.slint — official async-io example (spawn_local + async_compat + reqwest)
- https://github.com/slint-ui/slint/blob/master/CHANGELOG.md — 1.16.0 winit-default, 1.17.0 MSRV 1.92, 1.17.1 future-waker-after-event-loop-stop fix, 1.0.0 Result-returning `invoke_from_event_loop`
- https://crates.io/api/v1/crates/{slint,slint-build,tokio,reqwest,serde,serde_json,async-compat} — current version numbers (2026-08-08)
