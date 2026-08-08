# 04 — Search / add friend / approve friend

**What to build:** The user can grow their friend list from inside the app: search for users by name or uid (1007), send a friend request (1009), receive incoming friend-apply notifications (1011), and approve or ignore them (1013). An approved friend appears in the friend list. Because the live server's search (1007) currently returns `InvalidJson` for every request (known server bug, not to be fixed here), the search UI degrades gracefully: a failed search shows "search unavailable" rather than crashing or hanging.

**Blocked by:** 03 (Friend list + 1:1 text chat).

**Status:** ready-for-agent

- [ ] protocol module: Search (1007/1008), AddFriend (1009/1010), AuthFriend (1013/1014), friend-apply push (1011); unit-tested
- [ ] app-state reducer: search-result state, apply-sent feedback, incoming apply, approve/ignore transitions; unit-tested
- [ ] UI: add-friend dialog with a search field, result row, and "send request" button
- [ ] UI: incoming friend-apply notifications surface (banner/apply list) with approve/ignore actions
- [ ] UI: after approval the new friend appears in the friend list (updated in-app)
- [ ] UI: search failure (live-server InvalidJson or any error) shows a graceful "search unavailable" message
