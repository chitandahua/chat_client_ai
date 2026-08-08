slint::include_modules!();

mod app;
mod connection;
mod gate;
mod net_loop;
mod protocol;
mod ui_bridge;

use std::sync::{Arc, Mutex};

use app::AppState;
use net_loop::NetCmd;
use ui_bridge::push_ui_from_state;

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
                    rt.block_on(net_loop::run_network(weak, state, out_rx, server_host, username, password));
                })
                .expect("spawn net thread");
        }
    });

    ui.on_set_auth_mode({
        let ui = ui.as_weak();
        move |mode| {
            if let Some(ui) = ui.upgrade() {
                ui.set_auth_mode(mode);
                ui.set_login_status("".into());
            }
        }
    });

    // Register / reset-password / verify-code run a short-lived HTTP-only
    // thread (no chat socket), pushing the result back onto the UI thread.
    ui.on_request_verify_code({
        let ui = ui.as_weak();
        move || {
            let (server_host, email) = match ui.upgrade() {
                Some(ui) => (ui.get_server_host().to_string(), ui.get_email().to_string()),
                None => return,
            };
            let weak = ui.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
                let result = rt.block_on(gate::get_verify_code(&gate_url(&server_host), &email));
                let msg = match result {
                    // The server's mailer is not wired up, so it echoes a
                    // placeholder code; surface it so the flow is usable in test.
                    Ok(info) if info.code.is_empty() => format!("验证码已发送至 {}", info.email),
                    Ok(info) => format!("验证码: {} (测试环境,请勿外传)", info.code),
                    Err(e) => format!("获取验证码失败: {e}"),
                };
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_login_status(msg.into()));
            });
        }
    });

    ui.on_do_register({
        let ui = ui.as_weak();
        move || {
            let fields = match ui.upgrade() {
                Some(ui) => (
                    ui.get_server_host().to_string(),
                    ui.get_username().to_string(),
                    ui.get_email().to_string(),
                    ui.get_password().to_string(),
                    ui.get_confirm_password().to_string(),
                    ui.get_verify_code().to_string(),
                ),
                None => return,
            };
            let (server_host, user, email, passwd, confirm, code) = fields;
            if passwd != confirm {
                if let Some(ui) = ui.upgrade() {
                    ui.set_login_status("两次密码不一致".into());
                }
                return;
            }
            let weak = ui.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
                let result = rt.block_on(gate::register(
                    &gate_url(&server_host), &user, &email, &passwd, &confirm, &code,
                ));
                let msg = match result {
                    Ok(()) => "注册成功,请登录".to_string(),
                    Err(e) => format!("注册失败: {e}"),
                };
                let switch_to_login = msg.starts_with("注册成功");
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_login_status(msg.into());
                    if switch_to_login {
                        ui.set_auth_mode(0);
                    }
                });
            });
        }
    });

    ui.on_do_reset_password({
        let ui = ui.as_weak();
        move || {
            let fields = match ui.upgrade() {
                Some(ui) => (
                    ui.get_server_host().to_string(),
                    ui.get_username().to_string(),
                    ui.get_email().to_string(),
                    ui.get_password().to_string(),
                    ui.get_verify_code().to_string(),
                ),
                None => return,
            };
            let (server_host, user, email, passwd, code) = fields;
            let weak = ui.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
                let result = rt.block_on(gate::reset_password(&gate_url(&server_host), &user, &email, &passwd, &code));
                let msg = match result {
                    Ok(()) => "密码已重置,请登录".to_string(),
                    Err(e) => format!("重置失败: {e}"),
                };
                let switch_to_login = msg.starts_with("密码已重置");
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_login_status(msg.into());
                    if switch_to_login {
                        ui.set_auth_mode(0);
                    }
                });
            });
        }
    });

    ui.on_select_friend({
        let state = Arc::clone(&state);
        let ui = ui.as_weak();
        move |friend| {
            let mut s = state.lock().unwrap();
            s.open_conversation(friend.as_str());
            if let Some(ui) = ui.upgrade() {
                ui.set_selected_friend(friend.into());
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
                s.friend_uid(&friend).unwrap_or(0)
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
                s.search_uid().unwrap_or(0)
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

/// Build a gate base URL from a "host:port" server string.
fn gate_url(server_host: &str) -> String {
    match net_loop::split_host_port(server_host) {
        Some((host, port)) if port != 0 => format!("http://{host}:{port}"),
        _ => String::new(),
    }
}
