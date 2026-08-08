# Stand up the chat_project backend stack locally

Blocked by:
Type: task
Status: open

## Question

Get the `chat_project` backend running on this machine so the GUI prototype and protocol module can be validated against a live server: MySQL (schema in `sql/`), Redis, then verify/status/gate/chat servers. Requires resolving build+run steps from `chat_project/README.md`, `CMakeLists.txt`, and `config/*`. Records resulting facts later tickets depend on: which services are up, host/ports, and test accounts (uid/token) to log in with. Where the agent can drive it alone (AFK) it should; where it needs credentials or a running MySQL/Redis the human provides, hand a precise checklist (HITL).
