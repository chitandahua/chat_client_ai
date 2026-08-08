# 02 — Login flow end-to-end

**What to build:** A user can enter server host:port, username and password and log in for real: the app calls gate `POST /user_login`, gets back `{id, user, token, host, port}`, opens a TCP connection to the returned chat server, sends frame `1005` with `{"uid","token"}`, and on success shows the main window with the friend list (and pending applies) from the `1006` response. A failed login shows a clear error (wrong password, unreachable server, invalid token) and keeps the user on the login screen.

**Blocked by:** 01 (Scaffold + Slint window boots).

**Status:** ready-for-agent

- [ ] protocol module encodes/decodes the `[id:u32 BE][len:u16 BE][json]` frame; unit-tested (Seam 1)
- [ ] protocol module has typed LoginRequest (1005) / LoginResponse (1006); unit-tested
- [ ] gate client performs `POST /user_login` and parses `{id,user,token,host,port}` (Seam: gate HTTP)
- [ ] connection module connects, sends 1005, reads 1006 (Seam 3: integration vs mock or live server)
- [ ] app-state reducer has Login success/failure transitions; unit-tested (Seam 2)
- [ ] UI: on success the main window shows the friend list from the login response
- [ ] UI: login failure shows a readable error and stays on the login screen
- [ ] Login within the server's 10s deadline (framing sends uid+token promptly after connect)
