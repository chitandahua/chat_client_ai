# chat_project backend observations & suspected bugs

Recorded 2026-08-08 while standing up the backend for the Rust GUI effort. **Not fixed** — this is a separate effort's work; the GUI must tolerate the server as-is.

## Running stack (all localhost, empty mysql/redis user+pass as configured)

| Service | Port | Status |
|---|---|---|
| MariaDB (system, datadir `/var/lib/mysql`) | 3306 | up; db `chat_project`, tables `user`, `friend`, `friend_apply` |
| Redis | 6379 | up |
| gate_server | 10086 | up; HTTP `POST /user_login` |
| verify_server | 10087 | up |
| status_server | 10088 | up |
| chat_server (chat_server1.json) | 18080 | up |
| chat_server (chat_server2.json) | 18081 | up |

Test accounts in `user`: id 1 `aaa`/pwd `22`, id 3 `bbb`/pwd `456`, id 4 `ssss`/pwd `555`.

## Verified working (via Python raw-socket client)

- Gate login: `POST http://127.0.0.1:10086/user_login` with `{"user":"ssss","passwd":"555"}` → `{"data":{"host":"127.0.0.1","id":4,"port":18080,"token":"<uuid>","user":"ssss"},...}`.
- TCP login: frame id `1005`, body `{"uid":4,"token":"<token>"}` → `1006` with `data.{uid,token,name,friend_list,apply_list}`.
- Text chat: frame `1017` `{"fromuid":4,"touid":1,"text_array":[{"msgid":1,"content":"hi"}]}` → `1018` `{"error":0,"message":""}`.
- Wire framing confirmed exact: `[id: u32 BE][len: u16 BE][json body]`.

## Suspected bugs (to write tests for / review later)

1. **Search user (1007) always fails with `InvalidJson`.** — **FIXED 2026-08-08.** Root cause: `chat_server/chat_server.hpp:165` `for (MsgNode read_msg;;)` constructs the `MsgNode` once and reuses it across the read loop; the `data_` buffer is only zeroed at construction, so bytes from a previous, longer request remain past the new body. Any message shorter than the previous one parses trailing residue (`{"name":"aaa"}` became `{"name":"aaa"}e...` → `parse error at column 16`). Proof: on the same connection, a search with a body LONGER than the login body (80 > 59 bytes) returned `UserNotFound` normally while a SHORT one returned `InvalidJson`. Fix: construct `MsgNode read_msg;` inside the loop so the buffer is re-zeroed each iteration. Verified after rebuild: search by name/uid, add-friend (1009), auth-friend (1013) all now succeed, and the full aaa↔bbb friend+text flow works (1011 apply push, 1015 auth push, 1019 text push).
2. **Text-chat same-server delivery uses wrong notify id.** `chat_server/handle_message.cpp:520` delivers inbound text to a same-server recipient as `MessageId::NotifyAuthFriend` (1015) instead of `NotifyTextChatMsg` (1019). Cross-server path uses 1019 correctly. A client must accept text payloads on both 1015 and 1019.
3. **No friend-list refresh endpoint.** Friend list + pending applies are only returned in the login response (1006). No message type fetches them later.
4. **Offline messages dropped.** Chat is relay-only; `text_chat_msg` returns Success even if the target isn't online (no persistence).
5. **`get_response_id` gap.** `message_common.hpp` only maps Login/Search/AddFriend; AuthFriend and TextChat fall through to `InvalidRequest` (1500) on the exception-path error responses — normal handler responses are still correctly numbered (1014/1018).
6. **Stale test file** `chat_client/friend_auth.txt` uses id `1012`; the real AuthFriend request id is `1013`.
7. **Auth ownership checks commented out** in add_friend / auth_friend / text_chat_msg (`handle_message.cpp:329-331, 401-403, 492-494`) — server trusts whatever uid is in the body.

## Operational notes

- Binaries were pre-built in `chat_project/build`; servers take the config path as argv[1].
- `chat_project` is not yet tracked by git (many untracked files incl. `docs/desc.md`).
- MySQL/MariaDB accepts connections with **empty username** on `chat_project` (matches the empty `user`/`pass` in the configs).
