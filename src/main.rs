slint::include_modules!();

mod app;
mod connection;
mod gate;
mod protocol;

use std::sync::{Arc, Mutex};

use app::{AppState, Friend, FriendApply};
use connection::Connection;
use protocol::{
    AddFriendRequest, AuthFriendRequest, Frame, IncomingText, LoginResponse, NotifyAddFriend,
    SearchRequest, SimpleResponse, TextChatData, TextChatRequest, UserInfo,
};

/// A request the UI sends to the network loop, executed on the live connection.
#[derive(Debug, Clone)]
enum NetCmd {
    SendText { touid: i64, text: String },
    Search { name: String },
    AddFriend { touid: i64 },
    ApproveApply { fromuid: i64 },
}

#[derive(Debug, Clone, PartialEq)]
enum SearchOutcome {
    Found(UserInfo),
    Unavailable(String),
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let state = Arc::new(Mutex::new(AppState::new()));

    // UI -> network channel for outgoing commands. UnboundedSender::send is
    // sync and callable from the UI thread; the network loop owns the receiver.
    let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NetCmd>();
    let out_tx = Arc::new(Mutex::new(out_tx));
    let out_rx = Arc::new(Mutex::new(Some(out_rx)));

    ui.on_do_login({
        let ui = ui.as_weak();
        let state = Arc::clone(&state);
        move || {
            {
                let mut s = state.lock().unwrap();
                s.begin_login();
                let label = s.login_status.label().to_string();
                if let Some(ui) = ui.upgrade() {
                    ui.set_login_status(label.into());
                }
            }

            let (server_host, username, password) = match ui.upgrade() {
                Some(ui) => (
                    ui.get_server_host().to_string(),
                    ui.get_username().to_string(),
                    ui.get_password().to_string(),
                ),
                None => return,
            };

            let weak = ui.clone();
            let state = Arc::clone(&state);
            let out_rx = Arc::clone(&out_rx);
            std::thread::Builder::new()
                .name("net".into())
                .spawn(move || {
                    let Some(out_rx) = out_rx.lock().unwrap().take() else {
                        return; // a net thread is already running — ignore repeat login clicks
                    };
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("build tokio runtime");
                    rt.block_on(run_network(weak, state, out_rx, server_host, username, password));
                })
                .expect("spawn net thread");
        }
    });

    ui.on_select_friend({
        let state = Arc::clone(&state);
        let ui = ui.as_weak();
        move |friend| {
            let mut s = state.lock().unwrap();
            s.open_conversation(friend.as_str());
            if let Some(ui) = ui.upgrade() {
                push_ui_from_state(&ui, &s);
            }
        }
    });

    ui.on_send_text({
        let state = Arc::clone(&state);
        let ui = ui.as_weak();
        let out_tx = Arc::clone(&out_tx);
        move |friend, text| {
            let friend = friend.to_string();
            let text = text.to_string();
            let touid = {
                let s = state.lock().unwrap();
                s.friends.iter().find(|f| f.name == friend).map(|f| f.id).unwrap_or(0)
            };
            {
                let mut s = state.lock().unwrap();
                s.sent_message(&friend, text.clone());
                if let Some(ui) = ui.upgrade() {
                    push_ui_from_state(&ui, &s);
                }
            }
            // If the friend's uid is unknown (0 — e.g. a friend added via a push
            // whose uid never got learned), surface it rather than dropping.
            if touid == 0 {
                if let Some(ui) = ui.upgrade() {
                    ui.set_login_status(format!("无法发送给 {friend}:好友 uid 未知").into());
                }
                return;
            }
            if let Some(ui) = ui.upgrade() {
                ui.set_login_status("".into());
            }
            let _ = out_tx.lock().unwrap().send(NetCmd::SendText { touid, text });
        }
    });

    ui.on_search_user({
        let ui = ui.as_weak();
        let out_tx = Arc::clone(&out_tx);
        move |name| {
            if name.trim().is_empty() {
                return;
            }
            if let Some(ui) = ui.upgrade() {
                ui.set_search_in_progress(true);
                ui.set_search_status("".into());
                ui.set_search_result("".into());
            }
            let _ = out_tx.lock().unwrap().send(NetCmd::Search { name: name.trim().to_string() });
        }
    });

    ui.on_add_friend({
        let ui = ui.as_weak();
        let state = Arc::clone(&state);
        let out_tx = Arc::clone(&out_tx);
        move || {
            let touid = {
                let s = state.lock().unwrap();
                s.search_result.as_ref().map(|f| f.id).unwrap_or(0)
            };
            if touid == 0 {
                if let Some(ui) = ui.upgrade() {
                    ui.set_search_status("无法添加:缺少用户 uid".into());
                }
                return;
            }
            let _ = out_tx.lock().unwrap().send(NetCmd::AddFriend { touid });
        }
    });

    ui.on_approve_apply({
        let state = Arc::clone(&state);
        let out_tx = Arc::clone(&out_tx);
        move |name| {
            let fromuid = {
                let s = state.lock().unwrap();
                s.approve_apply_uid(&name).unwrap_or(0)
            };
            if fromuid == 0 {
                return;
            }
            let _ = out_tx.lock().unwrap().send(NetCmd::ApproveApply { fromuid });
        }
    });

    ui.on_reject_apply({
        let state = Arc::clone(&state);
        let ui = ui.as_weak();
        move |name| {
            let mut s = state.lock().unwrap();
            s.reject_apply(&name);
            if let Some(ui) = ui.upgrade() {
                push_ui_from_state(&ui, &s);
            }
        }
    });

    ui.run()
}

/// Push reducer state into the Slint properties. Call on the UI thread only.
fn push_ui_from_state(ui: &MainWindow, s: &AppState) {
    let selected = ui.get_selected_friend().to_string();

    let messages: Vec<slint::SharedString> = s
        .conversations
        .iter()
        .find(|c| c.friend == selected)
        .map(|c| {
            c.messages
                .iter()
                .map(|m| {
                    let prefix = if m.mine { "我: ".to_string() } else { format!("{selected}: ") };
                    slint::SharedString::from(format!("{prefix}{}", m.text))
                })
                .collect()
        })
        .unwrap_or_default();

    let unread_flags: Vec<i32> = s
        .friends
        .iter()
        .map(|f| if s.unread.iter().any(|u| u == &f.name) { 1 } else { 0 })
        .collect();

    let apply_names: Vec<slint::SharedString> =
        s.applies.iter().map(|a| slint::SharedString::from(a.name.clone())).collect();

    let friend_names: Vec<slint::SharedString> =
        s.friends.iter().map(|f| slint::SharedString::from(f.name.clone())).collect();

    ui.set_friend_names(std::rc::Rc::new(slint::VecModel::from(friend_names)).into());
    ui.set_chat_messages(std::rc::Rc::new(slint::VecModel::from(messages)).into());
    ui.set_unread_flags(std::rc::Rc::new(slint::VecModel::from(unread_flags)).into());
    ui.set_apply_names(std::rc::Rc::new(slint::VecModel::from(apply_names)).into());
}

/// Network thread: gate login -> TCP login -> persistent connection.
/// Reads incoming pushes AND outgoing text from the UI, both on the socket.
async fn run_network(
    ui: slint::Weak<MainWindow>,
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

    let mut out_rx = out_rx;
    loop {
        tokio::select! {
            res = conn.recv() => {
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
                handle_incoming_frame(&ui, Arc::clone(&state), &frame);
            }
            cmd = out_rx.recv() => {
                let Some(cmd) = cmd else { return };
                handle_net_cmd(&ui, Arc::clone(&state), &mut conn, cmd).await;
            }
        }
    }
}

/// Execute one UI-initiated command on the live connection, then consume the
/// single matching response frame (skipping any interleaved pushes).
async fn handle_net_cmd(
    ui: &slint::Weak<MainWindow>,
    state: Arc<Mutex<AppState>>,
    conn: &mut Connection,
    cmd: NetCmd,
) {
    let (frame_to_send, expected_id) = match &cmd {
        NetCmd::SendText { touid, text } => {
            let fromuid = state.lock().unwrap().my_uid;
            let req = TextChatRequest {
                fromuid,
                touid: *touid,
                text_array: vec![TextChatData { msgid: 1, content: text.clone() }],
            };
            (Frame::new(protocol::TEXT_CHAT_REQUEST, serde_json::to_vec(&req).unwrap()), protocol::TEXT_CHAT_RESPONSE)
        }
        NetCmd::Search { name } => {
            let req = SearchRequest::by_name(name.clone());
            (Frame::new(protocol::SEARCH_REQUEST, serde_json::to_vec(&req).unwrap()), protocol::SEARCH_RESPONSE)
        }
        NetCmd::AddFriend { touid } => {
            let my_uid = state.lock().unwrap().my_uid;
            let req = AddFriendRequest { uid: my_uid, touid: *touid };
            (Frame::new(protocol::ADD_FRIEND_REQUEST, serde_json::to_vec(&req).unwrap()), protocol::ADD_FRIEND_RESPONSE)
        }
        NetCmd::ApproveApply { fromuid } => {
            let my_uid = state.lock().unwrap().my_uid;
            let req = AuthFriendRequest { fromuid: my_uid, touid: *fromuid };
            (Frame::new(protocol::AUTH_FRIEND_REQUEST, serde_json::to_vec(&req).unwrap()), protocol::AUTH_FRIEND_RESPONSE)
        }
    };

    if let Err(e) = conn.send(&frame_to_send).await {
        let _ = ui.upgrade_in_event_loop(move |ui| {
            ui.set_search_status(format!("发送失败: {e}").into());
        });
        return;
    }

    // Wait for the response, skipping interleaved push frames.
    let timeout = std::time::Duration::from_secs(5);
    loop {
        let frame = match tokio::time::timeout(timeout, conn.recv()).await {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => {
                let msg = format!("连接断开: {e}");
                let _ = ui.upgrade_in_event_loop(move |ui| {
                    state.lock().unwrap().login_failed(msg.clone());
                    ui.set_login_status(msg.into());
                });
                return;
            }
            Err(_) => {
                if let NetCmd::Search { .. } = cmd {
                    let _ = ui.upgrade_in_event_loop(move |ui| {
                        ui.set_search_in_progress(false);
                        ui.set_search_status("搜索超时".into());
                    });
                }
                return;
            }
        };

        if frame.id == expected_id {
            handle_response_frame(ui, state, &cmd, &frame).await;
            return;
        }
        // interleaved push — dispatch and keep waiting
        handle_incoming_frame(ui, Arc::clone(&state), &frame);
    }
}

/// Handle a response frame matching the outstanding command.
async fn handle_response_frame(
    ui: &slint::Weak<MainWindow>,
    state: Arc<Mutex<AppState>>,
    cmd: &NetCmd,
    frame: &Frame,
) {
    match cmd {
        NetCmd::SendText { .. } => {
            // text chat response 1018 — errors surfaced via a status; nothing to render
            let _ = frame;
        }
        NetCmd::Search { .. } => {
            let outcome = match protocol::SearchResponse::from_body(&frame.body) {
                Ok(protocol::SearchResponse::Found(user)) => SearchOutcome::Found(user),
                Ok(protocol::SearchResponse::Error(e)) if e.error != 0 => {
                    SearchOutcome::Unavailable("搜索不可用(服务端错误)".into())
                }
                _ => SearchOutcome::Unavailable("搜索无结果".into()),
            };

            match outcome {
                SearchOutcome::Found(user) => {
                    let is_friend = state.lock().unwrap().friends.iter().any(|f| f.id == user.id);
                    state.lock().unwrap().set_search_result(Some(Friend { id: user.id, name: user.name.clone() }));
                    let name = user.name.clone();
                    let _ = ui.upgrade_in_event_loop(move |ui| {
                        ui.set_search_in_progress(false);
                        ui.set_search_result(name.into());
                        ui.set_search_result_is_friend(is_friend);
                        ui.set_search_status("".into());
                    });
                }
                SearchOutcome::Unavailable(msg) => {
                    state.lock().unwrap().set_search_result(None);
                    let msg = msg.clone();
                    let _ = ui.upgrade_in_event_loop(move |ui| {
                        ui.set_search_in_progress(false);
                        ui.set_search_result("".into());
                        ui.set_search_result_is_friend(false);
                        ui.set_search_status(msg.into());
                    });
                }
            }
        }
        NetCmd::AddFriend { .. } => {
            let ok = serde_json::from_slice::<SimpleResponse>(&frame.body)
                .map(|r| r.is_ok())
                .unwrap_or(false);
            let msg = if ok { "申请已发送".to_string() } else { "加好友失败".to_string() };
            let _ = ui.upgrade_in_event_loop(move |ui| {
                ui.set_search_status(msg.into());
            });
        }
        NetCmd::ApproveApply { fromuid } => {
            let ok = serde_json::from_slice::<SimpleResponse>(&frame.body)
                .map(|r| r.is_ok())
                .unwrap_or(false);
            let fromuid = *fromuid;
            let state2 = Arc::clone(&state);
            let _ = ui.upgrade_in_event_loop(move |ui| {
                if ok {
                    // remove the apply and add the friend (reducer), then refresh the UI
                    state2.lock().unwrap().approve_apply(fromuid);
                    ui.set_apply_status("已同意".into());
                    let s = state2.lock().unwrap();
                    push_ui_from_state(&ui, &s);
                } else {
                    ui.set_apply_status("同意失败".into());
                }
            });
        }
    }
}

fn handle_incoming_frame(ui: &slint::Weak<MainWindow>, state: Arc<Mutex<AppState>>, frame: &Frame) {
    // Incoming friend-apply push (1011).
    if frame.id == protocol::NOTIFY_ADD_FRIEND {
        if let Ok(apply) = serde_json::from_slice::<NotifyAddFriend>(&frame.body) {
            let apply = apply.clone();
            let _ = ui.upgrade_in_event_loop(move |ui| {
                let mut s = state.lock().unwrap();
                s.apply_received(apply.applyuid, apply.name.clone());
                push_ui_from_state(&ui, &s);
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
        push_ui_from_state(&ui, &s);
    });
}

/// Gate + TCP login. On success the reducer holds the friend list and the
/// returned connection is live for the rest of the session.
async fn do_login(
    state: &Arc<Mutex<AppState>>,
    server_host: &str,
    username: &str,
    password: &str,
) -> Result<Connection, String> {
    let gate_url = match split_host_port(server_host) {
        Some((host, port)) if port != 0 => format!("http://{host}:{port}"),
        _ => return Err("请输入服务器地址,形如 127.0.0.1:10086".into()),
    };

    let gate_info = gate::user_login(&gate_url, username, password)
        .await
        .map_err(|e| format!("登录失败: {e}"))?;

    let mut conn = Connection::connect(&gate_info.host, gate_info.port)
        .await
        .map_err(|e| format!("无法连接聊天服务器: {e}"))?;

    let frame = conn.login(gate_info.id, &gate_info.token).await.map_err(|e| format!("登录失败: {e}"))?;

    if frame.id != protocol::LOGIN_RESPONSE {
        return Err(format!("登录响应 id 不符: {}", frame.id));
    }

    let resp: LoginResponse =
        serde_json::from_slice(&frame.body).map_err(|e| format!("登录响应解析失败: {e}"))?;

    if resp.status != 0 {
        return Err(format!("登录失败: {}", resp.message));
    }

    let data = resp.data.ok_or_else(|| "登录响应缺少数据".to_string())?;

    let applies: Vec<FriendApply> = data
        .apply_list
        .into_iter()
        .map(|a| FriendApply { from_uid: a.id, name: a.name })
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

fn split_host_port(input: &str) -> Option<(String, u16)> {
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
