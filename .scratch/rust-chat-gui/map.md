# Rust Chat GUI — Wayfinding Map

## Destination

A cross-platform (Linux + Windows) Rust + Slint desktop chat client for the existing `chat_project` server — full login (gate HTTP → chat TCP), friend list, search user, add friend, approve friend, 1:1 text chat — with every architecture, protocol, and UX decision resolved so an implementation session can build it. This map produces decisions, not code.

## Notes

- **Pipeline progress**: wayfinder map → spec at `spec.md` → implementation tickets at `.scratch/chat-client/issues/` (5 tracer bullets, `ready-for-agent`). Next: `/implement` then `/code-review`.
- **Domain**: `chat_project` wire protocol — raw TCP, frame `[id:u32 BE][len:u16 BE]` + JSON body. Login flow: HTTP `POST /user_login` on gate `10086` → `{id, user, token, host, port}` → TCP to chat_server (`18080`) → frame `1005` with `{"uid","token"}` within 10s. Feature ids: 1005 login / 1007 search / 1009 add friend / 1013 auth friend / 1017 text chat; pushes 1011 (add-friend apply), 1015 (auth), 1019 (text).
- **Server quirks to tolerate (client must not rely on fixes)**: incoming text may arrive as **1015 or 1019** (same-server delivery bug sends 1015); friend list + pending applies only come in the login response — no refresh endpoint; chat is relay-only, offline messages dropped; server drops the socket on auth failure / 10s login deadline.
- **Planning effort**: resolve decisions, produce no deliverable code. Research via `/research`; prototypes via `/prototype`; conversation via `/grilling` + `/domain-modeling`.
- **Standing decisions (from grilling)**: full gate login (Q5); search / add / approve in scope (Q6); Linux + Windows targets (Q7); **no** local chat-history persistence (Q8).
- **Tracker**: local markdown (GitHub configured in `docs/agents/` but `gh` not installed / no remote yet). Effort root `.scratch/rust-chat-gui/`.

## Decisions so far

<!-- the index — one line per closed ticket: enough to judge relevance, then zoom the link for the detail the ticket holds -->

- [Slint async event-loop integration patterns](issues/01-research-slint-async-integration.md) — slint 1.17.1; background multi-threaded tokio runtime + `Weak::upgrade_in_event_loop` for network→UI, `.slint` callbacks + mpsc for UI→network; no `AsyncComponentRunner`/`#[callback]`; spawn_local needs `async_compat::Compat`.
- [Windows build strategy for a Rust + Slint app](issues/02-research-windows-build-strategy.md) — winit is Windows-ready by default; build on a real Windows/MSVC box, or cargo-xwin / GNU-mingw cross from Linux; GNU can't use Skia → FemtoVG/software renderer; record `windows_subsystem`, VC redist vs `+crt-static`, `/STACK:8000000`.
- [Stand up the chat_project backend stack locally](issues/03-task-stand-up-backend.md) — full stack running locally (MariaDB 3306, Redis 6379, gate 10086, status 10088, chat 18080/18081); login + text-chat verified over the wire. 7 suspected server bugs recorded in [research/backend-bugs.md](research/backend-bugs.md) and NOT fixed (search 1007 InvalidJson, 1015-vs-1019 notify id, no friend-list refresh, offline drops, etc.) — separate tests/review effort later.

## Not yet specified

- Chat-message display details (timestamps, send status, ordering) — sharpens in the UX grilling.
- Connection lifecycle / error states beyond first connect (drop, re-login) — sharpens in the architecture grilling.
- How the friend list stays current after add/approve when the server only returns it at login — sharpens in the architecture grilling.

## Out of scope

- Local chat-history persistence — ruled out for v1 (Q8); returns only as a fresh effort.
- Server-side fixes (e.g. the 1015/1019 text-delivery quirk) — the GUI must tolerate the server as-is; server changes are a separate effort.
- Registration / verify-code / password-reset UI — not in the minimal feature set.
- Mobile platforms / macOS verification — Linux + Windows only.
