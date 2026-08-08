//! The network half of the GUI: owns the live chat-server connection, exposes
//! a deep `Client` interface for typed protocol requests, and dispatches
//! incoming pushes into the reducer + UI.
//!
//! This is a deep module: one `Client` behind a small typed interface, hiding
//! frame encode/decode, request/response correlation, push filtering, and
//! timeouts. It talks to the socket only through `NetConnection`, so the
//! dispatch core is unit-testable with a mock connection.

use std::sync::{Arc, Mutex};

use crate::protocol;
use crate::app::{AppState, Friend, FriendApply};
use crate::connection::{Connection, NetConnection};
use crate::protocol::{
    AddFriendRequest, AuthFriendRequest, Frame, IncomingText, LoginResponse, NotifyAddFriend,
    SearchRequest, SimpleResponse, TextChatData, TextChatRequest, TextChatResponse, UserInfo,
};

/// A request the UI sends to the network loop, executed on the live connection.
#[derive(Debug, Clone)]
pub enum NetCmd {
    SendText { touid: i64, text: String },
    Search { name: String },
    AddFriend { touid: i64 },
    ApproveApply { fromuid: i64 },
}

/// Deep client: one typed method per protocol request. Each method performs the
/// full send→correlate→timeout round trip and returns a typed result; the
/// caller never sees frames, push filtering, or timeouts.
pub struct Client<'a> {
    conn: &'a mut dyn NetConnection,
    my_uid: i64,
    timeout: std::time::Duration,
}

impl<'a> Client<'a> {
    pub fn new(conn: &'a mut dyn NetConnection, my_uid: i64) -> Self {
        Client { conn, my_uid, timeout: std::time::Duration::from_secs(5) }
    }

    pub async fn search(
        &mut self,
        name: &str,
        on_push: &mut dyn FnMut(&Frame),
    ) -> Result<UserInfo, SearchError> {
        let req = SearchRequest::by_name(name.to_string());
        let frame = self
            .request(protocol::SEARCH_REQUEST, protocol::SEARCH_RESPONSE, &req, on_push)
            .await?;
        match protocol::SearchResponse::from_body(&frame.body) {
            Ok(protocol::SearchResponse::Found(user)) => Ok(user),
            Ok(protocol::SearchResponse::Error(e)) if e.error != 0 => {
                Err(SearchError::Unavailable("搜索不可用(服务端错误)".into()))
            }
            _ => Err(SearchError::Unavailable("搜索无结果".into())),
        }
    }

    pub async fn send_text(
        &mut self,
        touid: i64,
        text: &str,
        on_push: &mut dyn FnMut(&Frame),
    ) -> Result<(), SearchError> {
        let req = TextChatRequest {
            fromuid: self.my_uid,
            touid,
            text_array: vec![TextChatData { msgid: 1, content: text.to_string() }],
        };
        let frame = self
            .request(protocol::TEXT_CHAT_REQUEST, protocol::TEXT_CHAT_RESPONSE, &req, on_push)
            .await?;
        let resp: TextChatResponse = serde_json::from_slice(&frame.body)
            .map_err(|_| SearchError::Unavailable("消息响应解析失败".into()))?;
        if resp.is_ok() {
            Ok(())
        } else {
            Err(SearchError::Unavailable(resp.message))
        }
    }

    pub async fn add_friend(
        &mut self,
        touid: i64,
        on_push: &mut dyn FnMut(&Frame),
    ) -> Result<(), SearchError> {
        let req = AddFriendRequest { uid: self.my_uid, touid };
        let frame = self
            .request(protocol::ADD_FRIEND_REQUEST, protocol::ADD_FRIEND_RESPONSE, &req, on_push)
            .await?;
        let resp: SimpleResponse = serde_json::from_slice(&frame.body)
            .map_err(|_| SearchError::Unavailable("响应解析失败".into()))?;
        if resp.is_ok() {
            Ok(())
        } else {
            Err(SearchError::Unavailable("加好友失败".into()))
        }
    }

    pub async fn approve(
        &mut self,
        fromuid: i64,
        on_push: &mut dyn FnMut(&Frame),
    ) -> Result<(), SearchError> {
        let req = AuthFriendRequest { fromuid: self.my_uid, touid: fromuid };
        let frame = self
            .request(protocol::AUTH_FRIEND_REQUEST, protocol::AUTH_FRIEND_RESPONSE, &req, on_push)
            .await?;
        let resp: SimpleResponse = serde_json::from_slice(&frame.body)
            .map_err(|_| SearchError::Unavailable("响应解析失败".into()))?;
        if resp.is_ok() {
            Ok(())
        } else {
            Err(SearchError::Unavailable("同意失败".into()))
        }
    }

    /// Send a request frame, then read frames until the matching response id
    /// arrives. Interleaved push frames are handed to `on_push`.
    async fn request<T: serde::Serialize>(
        &mut self,
        req_id: u32,
        resp_id: u32,
        body: &T,
        on_push: &mut dyn FnMut(&Frame),
    ) -> Result<Frame, SearchError> {
        let frame = Frame::new(req_id, serde_json::to_vec(body).map_err(|e| SearchError::Protocol(e.to_string()))?);
        self.conn
            .send(&frame)
            .await
            .map_err(|e| SearchError::Protocol(e.to_string()))?;

        loop {
            let frame = tokio::time::timeout(self.timeout, self.conn.recv())
                .await
                .map_err(|_| SearchError::Timeout)?
                .map_err(|e| SearchError::Protocol(e.to_string()))?;
            if frame.id == resp_id {
                return Ok(frame);
            }
            on_push(&frame);
        }
    }

    /// Read the next frame without correlation — used by the owning loop to
    /// pick up pushes. The socket is still owned by the client.
    pub async fn recv_push(&mut self) -> Result<Frame, SearchError> {
        self.conn
            .recv()
            .await
            .map_err(|e| SearchError::Protocol(e.to_string()))
    }
}

#[derive(Debug)]
pub enum SearchError {
    Unavailable(String),
    Protocol(String),
    Timeout,
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchError::Unavailable(m) | SearchError::Protocol(m) => write!(f, "{m}"),
            SearchError::Timeout => write!(f, "请求超时"),
        }
    }
}

/// Network thread: gate login -> TCP login -> persistent connection.
/// Owns the socket, runs the deep `Client` for commands, and dispatches
/// pushes into the reducer + UI.
pub async fn run_network(
    ui: slint::Weak<crate::MainWindow>,
    state: Arc<Mutex<AppState>>,
    out_rx: tokio::sync::mpsc::UnboundedReceiver<NetCmd>,
    server_host: String,
    username: String,
    password: String,
) {
    let mut conn = match do_login(&state, &server_host, &username, &password).await {
        Ok(conn) => conn,
        Err(msg) => {
            let _ = ui.upgrade_in_event_loop(move |ui| {
                state.lock().unwrap().login_failed(msg.clone());
                ui.set_login_status(msg.into());
            });
            return;
        }
    };

    let names: Vec<slint::SharedString> = state
        .lock()
        .unwrap()
        .friends
        .iter()
        .map(|f| slint::SharedString::from(f.name.clone()))
        .collect();
    let _ = ui.upgrade_in_event_loop(move |ui| {
        ui.set_logged_in(true);
        ui.set_friend_names(std::rc::Rc::new(slint::VecModel::from(names)).into());
        ui.set_login_status("".into());
    });

    let my_uid = state.lock().unwrap().my_uid;
    let mut out_rx = out_rx;
    let mut client = Client::new(&mut conn, my_uid);
    loop {
        tokio::select! {
            res = client.recv_push() => {
                let frame = match res {
                    Ok(f) => f,
                    Err(e) => {
                        let msg = format!("连接断开: {e}");
                        let _ = ui.upgrade_in_event_loop(move |ui| {
                            state.lock().unwrap().login_failed(msg.clone());
                            ui.set_login_status(msg.into());
                        });
                        return;
                    }
                };
                handle_push(&ui, Arc::clone(&state), &frame);
            }
            cmd = out_rx.recv() => {
                let Some(cmd) = cmd else { return };
                execute_cmd(&ui, &state, &mut client, cmd).await;
            }
        }
    }
}

/// Execute one UI-initiated command through the deep Client.
async fn execute_cmd(
    ui: &slint::Weak<crate::MainWindow>,
    state: &Arc<Mutex<AppState>>,
    client: &mut Client<'_>,
    cmd: NetCmd,
) {
    // Interleaved pushes arriving while a command awaits its response are
    // dispatched here, mirroring the main loop.
    let mut on_push = |frame: &Frame| {
        let state = Arc::clone(state);
        handle_push(ui, state, frame);
    };
    let on_push = &mut on_push;

    match cmd {
        NetCmd::SendText { touid, text } => {
            let _ = client.send_text(touid, &text, on_push).await;
        }
        NetCmd::Search { name } => {
            match client.search(&name, on_push).await {
                Ok(user) => {
                    let is_friend = state.lock().unwrap().is_friend(user.id);
                    state.lock().unwrap().set_search_result(Some(Friend::new(user.id, user.name.clone())));
                    let name = user.name.clone();
                    let _ = ui.upgrade_in_event_loop(move |ui| {
                        ui.set_add_search_in_progress(false);
                        ui.set_add_search_result(name.into());
                        ui.set_add_search_result_is_friend(is_friend);
                        ui.set_add_search_status("".into());
                    });
                }
                Err(e) => {
                    state.lock().unwrap().set_search_result(None);
                    let msg = e.to_string();
                    let _ = ui.upgrade_in_event_loop(move |ui| {
                        ui.set_add_search_in_progress(false);
                        ui.set_add_search_result("".into());
                        ui.set_add_search_result_is_friend(false);
                        ui.set_add_search_status(msg.into());
                    });
                }
            }
        }
        NetCmd::AddFriend { touid } => {
            let msg = match client.add_friend(touid, on_push).await {
                Ok(()) => "申请已发送".to_string(),
                Err(e) => format!("加好友失败: {e}"),
            };
            let _ = ui.upgrade_in_event_loop(move |ui| {
                ui.set_add_search_status(msg.into());
            });
        }
        NetCmd::ApproveApply { fromuid } => {
            let ok = client.approve(fromuid, on_push).await.is_ok();
            let state2 = Arc::clone(state);
            let _ = ui.upgrade_in_event_loop(move |ui| {
                if ok {
                    state2.lock().unwrap().approve_apply(fromuid);
                    ui.set_apply_status("已同意".into());
                    let s = state2.lock().unwrap();
                    crate::ui_bridge::push_ui_from_state(&ui, &s);
                } else {
                    ui.set_apply_status("同意失败".into());
                }
            });
        }
    }
}

/// Dispatch an incoming push into the reducer + UI.
fn handle_push(ui: &slint::Weak<crate::MainWindow>, state: Arc<Mutex<AppState>>, frame: &Frame) {
    // Incoming friend-apply push (1011).
    if frame.id == protocol::NOTIFY_ADD_FRIEND {
        if let Ok(apply) = serde_json::from_slice::<NotifyAddFriend>(&frame.body) {
            let apply = apply.clone();
            let _ = ui.upgrade_in_event_loop(move |ui| {
                let mut s = state.lock().unwrap();
                s.apply_received(apply.applyuid, apply.name.clone());
                crate::ui_bridge::push_ui_from_state(&ui, &s);
            });
        }
        return;
    }

    // Incoming text may arrive as 1015 (server bug) or 1019.
    if frame.id != protocol::NOTIFY_TEXT_CHAT && frame.id != protocol::NOTIFY_AUTH_FRIEND {
        return;
    }
    let text: IncomingText = match serde_json::from_slice(&frame.body) {
        Ok(t) => t,
        Err(_) => return,
    };

    {
        let mut s = state.lock().unwrap();
        for item in &text.text_array {
            // learns the sender's uid when unambiguous, then routes + marks unread
            s.receive_push(text.fromuid, item.content.clone());
        }
    }

    let _ = ui.upgrade_in_event_loop(move |ui| {
        let s = state.lock().unwrap();
        crate::ui_bridge::push_ui_from_state(&ui, &s);
    });
}

/// Gate + TCP login. On success the reducer holds the friend list and the
/// returned connection is live for the rest of the session.
pub async fn do_login(
    state: &Arc<Mutex<AppState>>,
    server_host: &str,
    username: &str,
    password: &str,
) -> Result<Connection, String> {
    let gate_url = match split_host_port(server_host) {
        Some((host, port)) if port != 0 => format!("http://{host}:{port}"),
        _ => return Err("请输入服务器地址,形如 127.0.0.1:10086".into()),
    };

    let gate_info = crate::gate::user_login(&gate_url, username, password)
        .await
        .map_err(|e| format!("登录失败: {e}"))?;

    let mut conn = Connection::connect(&gate_info.host, gate_info.port)
        .await
        .map_err(|e| format!("无法连接聊天服务器: {e}"))?;

    let frame = conn
        .login(gate_info.id, &gate_info.token)
        .await
        .map_err(|e| format!("登录失败: {e}"))?;

    if frame.id != protocol::LOGIN_RESPONSE {
        return Err(format!("登录响应 id 不符: {}", frame.id));
    }

    let resp: LoginResponse =
        serde_json::from_slice(&frame.body).map_err(|e| format!("登录响应解析失败: {e}"))?;

    if resp.status != 0 {
        return Err(format!("登录失败: {}", resp.message));
    }

    let data = resp.data.ok_or_else(|| "登录响应缺少数据".to_string())?;

    // Only pending applies (status 0) show as "requesting to be your friend".
    // Approved ones (status 1) are already friends and must not be listed.
    let applies: Vec<FriendApply> = data
        .apply_list
        .into_iter()
        .map(|a| FriendApply::new(a.id, a.name, a.status))
        .collect();
    let friends: Vec<Friend> = data
        .friend_list
        .into_iter()
        .map(|f| Friend::new(f.id, f.name))
        .collect();
    {
        let mut s = state.lock().unwrap();
        s.login_succeeded(data.uid, friends);
        s.seed_applies(applies);
    }

    Ok(conn)
}

pub fn split_host_port(input: &str) -> Option<(String, u16)> {
    let trimmed = input.trim();
    let (host, port) = trimmed.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    if host.is_empty() {
        return None;
    }
    Some((host.trim_matches(['[', ']']).to_string(), port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::ConnectionError;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::pin::Pin;

    /// Programmable mock connection: a queue of incoming frames and a capture
    /// of what was sent. Lets the deep `Client` be tested without a socket.
    struct MockConnection {
        incoming: VecDeque<Frame>,
        sent: Vec<Frame>,
    }

    impl MockConnection {
        fn new(incoming: Vec<Frame>) -> Self {
            Self { incoming: incoming.into(), sent: Vec::new() }
        }
    }

    impl NetConnection for MockConnection {
        fn send(&mut self, frame: &Frame) -> Pin<Box<dyn Future<Output = Result<(), ConnectionError>> + Send + '_>> {
            let frame = frame.clone();
            Box::pin(async move {
                self.sent.push(frame);
                Ok(())
            })
        }
        fn recv(&mut self) -> Pin<Box<dyn Future<Output = Result<Frame, ConnectionError>> + Send + '_>> {
            Box::pin(async move {
                // Empty queue: block forever so the caller's timeout fires.
                if self.incoming.is_empty() {
                    std::future::pending::<()>().await;
                }
                Ok(self.incoming.pop_front().expect("non-empty"))
            })
        }
    }

    fn frame(id: u32, body: &str) -> Frame {
        Frame::new(id, body.as_bytes().to_vec())
    }

    #[tokio::test]
    async fn client_search_returns_found_user() {
        let user = r#"{"id":1,"name":"aaa","email":"x"}"#.to_string();
        let mut conn = MockConnection::new(vec![frame(protocol::SEARCH_RESPONSE, &user)]);
        let mut client = Client::new(&mut conn, 4);

        let result = client.search("aaa", &mut |_| {}).await.unwrap();
        assert_eq!(result.id, 1);
        assert_eq!(result.name, "aaa");
        // sent the right request id
        assert_eq!(conn.sent[0].id, protocol::SEARCH_REQUEST);
    }

    #[tokio::test]
    async fn client_search_surfaces_server_error() {
        let err = r#"{"error":1011,"message":"UserNotFound"}"#.to_string();
        let mut conn = MockConnection::new(vec![frame(protocol::SEARCH_RESPONSE, &err)]);
        let mut client = Client::new(&mut conn, 4);

        let result = client.search("nope", &mut |_| {}).await;
        assert!(matches!(result, Err(SearchError::Unavailable(_))));
    }

    #[tokio::test]
    async fn client_send_text_succeeds_on_ok_response() {
        let ok = r#"{"error":0,"message":""}"#.to_string();
        let mut conn = MockConnection::new(vec![frame(protocol::TEXT_CHAT_RESPONSE, &ok)]);
        let mut client = Client::new(&mut conn, 4);

        client.send_text(1, "hi", &mut |_| {}).await.unwrap();
        assert_eq!(conn.sent[0].id, protocol::TEXT_CHAT_REQUEST);
    }

    #[tokio::test]
    async fn client_add_friend_and_approve_roundtrip() {
        let ok = r#"{"error":0,"message":"Success"}"#.to_string();
        let mut conn = MockConnection::new(vec![
            frame(protocol::ADD_FRIEND_RESPONSE, &ok),
            frame(protocol::AUTH_FRIEND_RESPONSE, &ok),
        ]);
        let mut client = Client::new(&mut conn, 4);

        client.add_friend(3, &mut |_| {}).await.unwrap();
        client.approve(1, &mut |_| {}).await.unwrap();
        assert_eq!(conn.sent[0].id, protocol::ADD_FRIEND_REQUEST);
        assert_eq!(conn.sent[1].id, protocol::AUTH_FRIEND_REQUEST);
    }

    #[tokio::test]
    async fn client_times_out_when_no_response() {
        let mut conn = MockConnection::new(vec![]);
        let mut client = Client::new(&mut conn, 4);
        client.timeout = std::time::Duration::from_millis(20);

        let result = client.search("aaa", &mut |_| {}).await;
        assert!(matches!(result, Err(SearchError::Timeout)));
    }

    #[test]
    fn split_host_port_parses_address() {
        assert_eq!(split_host_port("127.0.0.1:10086"), Some(("127.0.0.1".into(), 10086)));
        assert_eq!(split_host_port("localhost:18080"), Some(("localhost".into(), 18080)));
        assert_eq!(split_host_port("no-port"), None);
        assert_eq!(split_host_port(""), None);
        assert_eq!(split_host_port(":10086"), None);
    }

    // Live-backend integration check: requires the chat_project stack running locally.
    // Run manually with: cargo test -- --ignored live_login
    #[tokio::test]
    #[ignore = "requires live chat_project backend on 127.0.0.1"]
    async fn live_login_flow() {
        let state = Arc::new(Mutex::new(AppState::new()));
        let outcome = do_login(&state, "127.0.0.1:10086", "ssss", "555").await;
        assert!(outcome.is_ok(), "expected success, got: {:?}", outcome.err());
    }
}
