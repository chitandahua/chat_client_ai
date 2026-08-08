# Stand up the chat_project backend stack locally

Blocked by:
Type: task
Status: resolved

## Question

Get the `chat_project` backend running on this machine so the GUI prototype and protocol module can be validated against a live server: MySQL (schema in `sql/`), Redis, then verify/status/gate/chat servers. Requires resolving build+run steps from `chat_project/README.md`, `CMakeLists.txt`, and `config/*`. Records resulting facts later tickets depend on: which services are up, host/ports, and test accounts (uid/token) to log in with. Where the agent can drive it alone (AFK) it should; where it needs credentials or a running MySQL/Redis the human provides, hand a precise checklist (HITL).

## Answer

Full observations + suspected-bug list: `.scratch/rust-chat-gui/research/backend-bugs.md`.

- **Stack up** (all localhost): MariaDB 3306 (db `chat_project`), Redis 6379, gate 10086, verify 10087, status 10088, chat_server1 18080, chat_server2 18081. Test accounts: `ssss`/555 (uid 4), `aaa`/22 (uid 1), `bbb`/456 (uid 3).
- **Verified working**: gate HTTP `/user_login` returns `{id,user,token,host,port}`; TCP login 1005→1006 returns uid/token/name/friend_list/apply_list; text chat 1017→1018 returns success. Wire framing confirmed exact.
- **Suspected bugs recorded (NOT fixed — user will ask for tests/review separately)**: (1) search 1007 always returns InvalidJson even from a fresh rebuilt binary — framing verified correct on the wire, server-side parse fails at column 16 for a 9-byte body; (2) same-server text delivery uses notify id 1015 instead of 1019; (3) no friend-list refresh endpoint; (4) offline messages dropped (no persistence); (5) `get_response_id` gap for AuthFriend/TextChat exception paths; (6) stale test file uses wrong id 1012 vs 1013; (7) auth ownership checks commented out in mutating handlers.
- **GUI must tolerate**: accept text payloads on both 1015 and 1019; friend list only at login; search may need to work around the InvalidJson bug or it stays broken server-side.
