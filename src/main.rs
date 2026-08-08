slint::include_modules!();

mod app;
mod connection;
mod gate;
mod protocol;

use std::sync::{Arc, Mutex};

use app::{AppState, Friend};
use connection::Connection;
use protocol::{Frame, IncomingText, LoginResponse, TextChatData, TextChatRequest};

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let state = Arc::new(Mutex::new(AppState::new()));

    // UI -> network channel for outgoing text. UnboundedSender::send is sync
    // and callable from the UI thread; the network loop owns the receiver.
    let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<(String, String, i64)>();
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
                    let out_rx = out_rx.lock().unwrap().take().expect("net thread spawned once");
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
            // Server friend list carries no uid, so touid is unknown (0) and
            // the send is dropped rather than corrupting the conversation.
            if touid == 0 {
                return;
            }
            let _ = out_tx.lock().unwrap().send((friend, text, touid));
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

    ui.set_chat_messages(std::rc::Rc::new(slint::VecModel::from(messages)).into());
    ui.set_unread_flags(std::rc::Rc::new(slint::VecModel::from(unread_flags)).into());
}

/// Network thread: gate login -> TCP login -> persistent connection.
/// Reads incoming pushes AND outgoing text from the UI, both on the socket.
async fn run_network(
    ui: slint::Weak<MainWindow>,
    state: Arc<Mutex<AppState>>,
    out_rx: tokio::sync::mpsc::UnboundedReceiver<(String, String, i64)>,
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
                let Some((_friend, text, touid)) = cmd else { return };
                let msgid = 1;
                let fromuid = state.lock().unwrap().my_uid;
                let req = TextChatRequest {
                    fromuid,
                    touid,
                    text_array: vec![TextChatData { msgid, content: text.clone() }],
                };
                if let Ok(body) = serde_json::to_vec(&req) {
                    let _ = conn.send(&Frame::new(protocol::TEXT_CHAT_REQUEST, body)).await;
                }
            }
        }
    }
}

fn handle_incoming_frame(ui: &slint::Weak<MainWindow>, state: Arc<Mutex<AppState>>, frame: &Frame) {
    // Incoming text may arrive as 1015 (server bug) or 1019.
    if frame.id != protocol::NOTIFY_TEXT_CHAT && frame.id != protocol::NOTIFY_AUTH_FRIEND {
        return;
    }
    let text: IncomingText = match serde_json::from_slice(&frame.body) {
        Ok(t) => t,
        Err(_) => return,
    };

    let friend = {
        let s = state.lock().unwrap();
        s.friend_for_uid(text.fromuid)
    };
    {
        let mut s = state.lock().unwrap();
        for item in &text.text_array {
            s.received_message(&friend, item.content.clone());
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

    let friends: Vec<Friend> =
        data.friend_list.into_iter().map(|f| Friend::from_name(f.name)).collect();
    state.lock().unwrap().login_succeeded(data.uid, friends);

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
