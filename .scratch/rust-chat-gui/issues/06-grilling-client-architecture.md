# Client architecture & protocol module design

Blocked by: 01, 05
Type: grilling
Status: open

## Question

Settle the client's architecture so implementation can start: crate/module layout; the async model — how tokio tasks (TCP reader/writer, HTTP login) hand events to the Slint UI thread; the protocol module (frame encode/decode of `[id:u32 BE][len:u16 BE]` + JSON body, typed messages for 1005-1019, tolerant handling of the 1015-or-1019 text-delivery quirk, error-envelope mapping); and app-state management for login / friend list / open chat / incoming messages — including how the friend list stays current with no refresh endpoint. Uses `/grilling` + `/domain-modeling`. The answer is the backbone the implementer builds from.
