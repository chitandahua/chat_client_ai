# 03 — Friend list + 1:1 text chat

**What to build:** From the main window the user can pick a friend and read/send 1:1 text messages in real time. Sending uses frame `1017` (`{"fromuid","touid","text_array":[{"msgid","content"}]}`) and receives `1018`. Incoming messages arrive over the same connection and are handled whether they arrive as `1015` OR `1019` (both may carry a `text_array`) — the spec's documented server quirk. A friend not currently open shows an unread marker when a message arrives for them.

**Blocked by:** 02 (Login flow end-to-end).

**Status:** ready-for-agent

- [ ] protocol module: TextChatRequest (1017) / TextChatResponse (1018) and a shared "incoming text" payload parseable from both 1015 and 1019; unit-tested with both ids
- [ ] protocol module: push types for friend-apply (1011) — decoded even if surfaced later
- [ ] app-state reducer: SendText appends to the open chat; IncomingText appends if that friend's chat is open, else marks the friend unread; unit-tested
- [ ] connection streams inbound frames and dispatches to the reducer/UI
- [ ] UI: chat pane shows sent and received messages for the selected friend
- [ ] UI: sending a message appends it to the open chat
- [ ] UI: a friend with unread messages is visually marked; opening them clears the marker
