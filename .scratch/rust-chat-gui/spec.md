# Rust Chat GUI — Spec

Status: ready-for-agent

## Problem Statement

The user has an existing C++ chat platform (`chat_project` — gate/status/verify/chat servers, MySQL + Redis, raw-TCP JSON protocol) but only a line-based CLI client. They want a simple desktop chat GUI so they can actually use the platform like a real chat app. It must be written in Rust with the Slint UI framework, run on both Linux and Windows, and connect to the existing `chat_project` servers without modifying them.

## Solution

A Rust + Slint desktop chat client that:

1. Shows a **login screen** (server host:port, username, password) that calls the gate server's HTTP `/user_login`, gets back `{id, user, token, host, port}`, then opens a TCP connection to the returned chat server and logs in with message id `1005`.
2. Shows the **main window**: a friend list (from the login response) on the left and a chat pane on the right, for 1:1 text chat with any friend.
3. Supports **search user** (1007), **add friend** (1009), and **approve friend** (1013) — so the friend list can grow from within the app.
4. Receives incoming events (friend-apply 1011, friend-approved 1015, incoming text 1015-or-1019) pushed over the same TCP connection and surfaces them in the UI.

The client is cross-platform (Linux + Windows) using Slint's winit backend. It keeps no local chat history (messages live in memory for the session only). It tolerates the server's known quirks rather than depending on server fixes.

## User Stories

1. As a user, I want to enter my server address, username and password and log in, so that I can use the chat app.
2. As a user, I want to see a clear error if login fails (wrong password, server unreachable, token invalid), so that I know what went wrong.
3. As a user, I want the app to connect to the chat server returned by the gate server, so that I don't have to know server internals.
4. As a user, I want to see my friend list after logging in, so that I can pick someone to talk to.
5. As a user, I want to see any pending friend-apply notifications (requests waiting for my approval), so that I can approve or ignore them.
6. As a user, I want to click a friend and see a chat pane for that friend, so that I can read and send messages.
7. As a user, I want to type a message and send it, so that my friend receives it in real time.
8. As a user, I want incoming messages to appear in the open chat as they arrive, so that the conversation flows live.
9. As a user, I want incoming messages for a friend I'm not currently viewing to be noticed (e.g. a marker on the friend), so that I don't miss them.
10. As a user, I want to search for other users by name or uid, so that I can find people to add.
11. As a user, I want to send a friend request to a search result, so that I can add them as a friend.
12. As a user, I want to receive friend requests from others, so that I can choose to become friends.
13. As a user, I want to approve or reject an incoming friend request, so that I control who becomes my friend.
14. As a user, I want the friend list to reflect newly added friends, so that I can start chatting with them.
15. As a user, I want the window to work on both Linux and Windows without code changes, so that I can use it on either OS.
16. As a user, I want the app to tolerate the server's text-delivery quirk (text may arrive as id 1015 or 1019), so that messages aren't lost when the server mislabels them.

## Implementation Decisions

### Architecture

- **Crate layout** (single crate, `src/`):
  - `main.rs` — boots the Slint UI + the network runtime, wires callbacks.
  - `protocol.rs` — pure, I/O-free: frame encode/decode (`[id: u32 BE][len: u16 BE]` + JSON body) and typed request/response/push structs for ids 1005–1019. This is the primary test seam.
  - `gate.rs` — HTTP client for `POST /user_login` (reqwest), returns `{id, user, token, host, port}`.
  - `connection.rs` — async (tokio) TCP connection to the chat server: `send(frame)` and an inbound event stream. Reads the 6-byte header then the body.
  - `app.rs` — app-state reducer: `Login`, `Friends`, `OpenChat { friend, messages }`, `PendingApplies`, plus actions (`SendText`, `IncomingText`, `ApplyReceived`, `ApprovalResult`, `SearchResult`...). Pure state transitions — the second test seam.
  - `ui/` — `.slint` markup: login screen, main window (friend list + chat pane), add-friend dialog, apply-notification surface.
- **Async / threading model** (from research ticket "Slint async event-loop integration patterns"):
  - Main thread creates the Slint component and runs `ui.run()`.
  - A **background multi-threaded tokio runtime** owns the socket + gate HTTP; `std::thread::spawn` it.
  - Network→UI: from tokio tasks call `Weak<Component>::upgrade_in_event_loop(|ui| ...)` (or `slint::invoke_from_event_loop`). Never touch Slint state from the tokio thread directly.
  - UI→network: `.slint` callbacks forward into the tokio task via `tokio::sync::mpsc` (`blocking_send`).
  - Short-lived ops (e.g. the login HTTP call) may use `slint::spawn_local(async_compat::Compat::new(fut))` on the UI thread.
  - Dependencies: slint `1.17.1`, slint-build `1.17.1`, tokio `1.53` (rt-multi-thread, net, io-util, sync, macros), reqwest `0.13` (json), serde/serde_json `1.0`, async-compat `0.2`.
- **Wire protocol facts** (from backend task + exploration; client must not assume server fixes):
  - Frame: `[id: u32 BE][body_len: u16 BE][body: JSON bytes]`, max body 1024.
  - Login flow: gate `POST /user_login` `{"user","passwd"}` → `data.{id,user,token,host,port}`; TCP frame `1005` `{"uid","token"}` within 10s → `1006` with `data.{uid,token,name,friend_list,apply_list}`.
  - Feature ids: `1007` search `{"uid"}` or `{"name"}`; `1009` add `{"uid","touid"}`; `1013` auth `{"fromuid","touid"}` (fromuid = approver); `1017` text `{"fromuid","touid","text_array":[{"msgid","content"}]}`.
  - Pushes: `1011` friend-apply `{"applyuid","name"}`; `1015` auth `{"fromuid","touid"}`; **text may arrive as `1015` OR `1019`** (same-server delivery bug sends 1015 with a `text_array`; cross-server uses 1019) — accept text payloads on both ids.
  - Friend list + pending applies come **only** in the login response (1006). No refresh endpoint.
  - Error envelope: `{"error":<code>,"message":"<name>"}`; 0 = success. Server closes the socket on auth failure / login deadline.
- **Search caveat (server bug, NOT fixed per decision)**: the live `chat_server` returns `{"error":1001,"message":"InvalidJson"}` for every 1007 even though the client framing is correct (reproduces after rebuild; suspected server parse bug). The GUI still implements the search flow and its UI; integration tests use a mock or a fixed server; the search feature is expected to fail against the current live server until that server bug is fixed. Flagged in the UI as "search unavailable" if the server keeps erroring — a graceful-failure path, not a workaround hack.
- **Windows build** (from research ticket "Windows build strategy"):
  - Primary path: build on a real Windows machine, MSVC (`x86_64-pc-windows-msvc`, VS 2022).
  - From Linux: cargo-xwin (MSVC) or GNU mingw cross; **GNU builds must use the FemtoVG or software renderer** (Skia needs MSVC on Windows). Enabling `renderer-femtovg` in the slint feature set.
  - Record in the build docs: `#![windows_subsystem = "windows"]` (no console), VC++ Redistributable for MSVC (or `-C target-feature=+crt-static` for a self-contained exe), and `/STACK:8000000` rustflags on msvc targets to avoid debug stack overflow.
- **UI layout**: three screens — login, main window (friend list left ~260px, chat pane right), add-friend dialog (search + result + send request). Incoming friend-apply notifications surface via a banner row and/or an apply marker; per-message and apply details are default choices to be confirmed in review (UX not yet grilled; see Further Notes).

## Testing Decisions

- **Good tests** exercise external behavior at a seam, not implementation details. Prefer the protocol module and the app-state reducer — they are pure, fast, and capture the domain logic. The connection seam is an integration test against a mock TCP server (or the live local chat_server) speaking the wire protocol. The Slint UI is not unit-tested — verified by running.
- **Seam 1 — protocol module (primary)**: unit tests encode a known frame from a typed message and decode raw bytes back to the same typed message; cover header endianness, body-length round-trips, the 1005/1006 login pair, 1007/1008 search, 1009/1010 add, 1013/1014 auth, 1017/1018 text, and the push ids 1011/1015/1019 — including **decoding a `text_array` from both 1015 and 1019**.
- **Seam 2 — app-state reducer**: unit tests for login success/failure, adding a friend, receiving a friend apply, approving, sending text (append to open chat), receiving text (append to open chat if selected, else mark the friend), and empty/missing friend-list handling.
- **Seam 3 — connection (integration)**: an in-process mock TCP server that speaks the wire format; the connection sends a frame and receives/decodes a response, and streams pushes. Where useful, the real local chat_server (see backend task) can serve the same purpose.
- **Prior art**: none yet — this is the first test suite in the repo. Follow Rust + cargo test conventions.

## Out of Scope

- Local chat-history persistence (messages are session-only).
- Server-side fixes to `chat_project` (search 1007 bug, 1015/1019 mislabel, offline-drop, no friend-list refresh) — the client tolerates them; fixing the server is a separate effort.
- Registration / verify-code / password-reset UI (server endpoints exist but are not in the minimal feature set).
- macOS verification — Linux + Windows only.
- Group chat, file/image transfer, message recall, unread-count persistence.
- Production packaging/installers beyond the standalone exe notes above.

## Further Notes

- Backend facts and suspected server bugs live at `.scratch/rust-chat-gui/research/backend-bugs.md`; protocol/servern internals reference in `chat_project` (`chat_server/msg_node.hpp`, `handle_message.cpp`, `message_common.hpp`).
- The two research tickets that informed this spec: "Slint async event-loop integration patterns" and "Windows build strategy" (both in `.scratch/rust-chat-gui/issues/`).
- Open UX choices (chat bubble layout, apply-notification placement, empty states) were not grilled — the implementation should make sensible defaults and code-review should flag them for a follow-up UX pass.
- A throwaway Slint prototype (3 layout variants) was begun at `prototype/` in this repo but is **abandoned/unfinished** — do not build on it; it is not part of this spec.
- Triage labels: `ready-for-agent` applied. Tracker: local markdown (GitHub configured in `docs/agents/` but `gh` not installed).
