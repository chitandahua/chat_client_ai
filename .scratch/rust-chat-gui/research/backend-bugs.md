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

1. **Search user (1007) always fails with `InvalidJson`.** Raw-socket tests with both `{"uid":1}` and `{"name":"aaa"}` (spaces and no spaces, fresh connections, after successful login) return `1008 {"error":1001,"message":"InvalidJson"}`. The wire capture shows the client sends exactly `00 00 03 ef 00 09 {"uid":1}` (id 1007, len 9) — framing is correct. Server log shows `nlohmann::json::parse` error `parse error at line 1, column 16 ... unexpected string literal; expected end of input` for a 9-byte body. Note: the shipped `chat_server` binary was stale (built 18:02, source touched 21:27); rebuilt from source and **still reproduces**. Suspect: `MsgNode::body()`/reader offset or the `search_user` parse path. Needs a test to pin down.
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
