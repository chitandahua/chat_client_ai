//! The network loop: owns the live chat-server connection, runs the deep
//! `Client` (in `client.rs`) for commands, and dispatches incoming pushes
//! into the reducer + UI.

use std::sync::{Arc, Mutex};

use crate::app::{AppState, Friend, FriendApply};
use crate::client::{Client, NetCmd};
use crate::connection::Connection;
use crate::protocol;
use crate::protocol::{Frame, IncomingText, LoginResponse, NotifyAddFriend};

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
    // collected by the Client and drained here — one dispatch path.
    match cmd {
        NetCmd::SendText { touid, text } => {
            let _ = client.send_text(touid, &text).await;
        }
        NetCmd::Search { name } => {
            match client.search(&name).await {
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
            let msg = match client.add_friend(touid).await {
                Ok(()) => "申请已发送".to_string(),
                Err(e) => format!("加好友失败: {e}"),
            };
            let _ = ui.upgrade_in_event_loop(move |ui| {
                ui.set_add_search_status(msg.into());
            });
        }
        NetCmd::ApproveApply { fromuid } => {
            let ok = client.approve(fromuid).await.is_ok();
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

    // dispatch any pushes collected while awaiting this command
    for frame in client.drain_pushes() {
        handle_push(ui, Arc::clone(state), &frame);
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