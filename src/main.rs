slint::include_modules!();

mod app;
mod connection;
mod gate;
mod protocol;

use std::sync::{Arc, Mutex};

use app::{AppState, Friend};
use connection::Connection;
use protocol::LoginResponse;

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let state = Arc::new(Mutex::new(AppState::new()));

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
            std::thread::Builder::new()
                .name("net".into())
                .spawn(move || {
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("build tokio runtime");
                    rt.block_on(run_login(weak, state, server_host, username, password));
                })
                .expect("spawn net thread");
        }
    });

    ui.run()
}

/// Run the login flow on a background tokio runtime, then push the result
/// onto the Slint UI thread, updating the reducer there.
async fn run_login(
    ui: slint::Weak<MainWindow>,
    state: Arc<Mutex<AppState>>,
    server_host: String,
    username: String,
    password: String,
) {
    let outcome = do_login(&server_host, &username, &password).await;

    let _ = ui.upgrade_in_event_loop(move |ui| {
        let mut s = state.lock().unwrap();
        match outcome {
            LoginOutcome { ok: true, message: _, friends } => {
                s.login_succeeded(friends);
                let names = s
                    .friends
                    .iter()
                    .map(|f| slint::SharedString::from(f.name.clone()))
                    .collect::<Vec<_>>();
                let model = std::rc::Rc::new(slint::VecModel::from(names));
                ui.set_logged_in(true);
                ui.set_friend_names(model.into());
                ui.set_login_status("".into());
            }
            LoginOutcome { ok: false, message, .. } => {
                s.login_failed(message.clone());
                ui.set_login_status(message.into());
            }
        }
    });
}

#[derive(Debug, Clone)]
struct LoginOutcome {
    ok: bool,
    message: String,
    friends: Vec<Friend>,
}

async fn do_login(server_host: &str, username: &str, password: &str) -> LoginOutcome {
    let gate_url = match split_host_port(server_host) {
        Some((host, port)) if port != 0 => format!("http://{host}:{port}"),
        _ => {
            return LoginOutcome {
                ok: false,
                message: "请输入服务器地址,形如 127.0.0.1:10086".into(),
                friends: Vec::new(),
            }
        }
    };

    let gate_info = match gate::user_login(&gate_url, username, password).await {
        Ok(info) => info,
        Err(e) => {
            return LoginOutcome { ok: false, message: format!("登录失败: {e}"), friends: Vec::new() };
        }
    };

    let mut conn = match Connection::connect(&gate_info.host, gate_info.port).await {
        Ok(c) => c,
        Err(e) => {
            return LoginOutcome {
                ok: false,
                message: format!("无法连接聊天服务器: {e}"),
                friends: Vec::new(),
            }
        }
    };

    let frame = match conn.login(gate_info.id, &gate_info.token).await {
        Ok(f) => f,
        Err(e) => {
            return LoginOutcome { ok: false, message: format!("登录失败: {e}"), friends: Vec::new() };
        }
    };

    if frame.id != protocol::LOGIN_RESPONSE {
        return LoginOutcome {
            ok: false,
            message: format!("登录响应 id 不符: {}", frame.id),
            friends: Vec::new(),
        };
    }

    let resp: LoginResponse = match serde_json::from_slice(&frame.body) {
        Ok(r) => r,
        Err(e) => {
            return LoginOutcome {
                ok: false,
                message: format!("登录响应解析失败: {e}"),
                friends: Vec::new(),
            }
        }
    };

    if resp.status != 0 {
        return LoginOutcome {
            ok: false,
            message: format!("登录失败: {}", resp.message),
            friends: Vec::new(),
        };
    }

    let data = match resp.data {
        Some(d) => d,
        None => {
            return LoginOutcome { ok: false, message: "登录响应缺少数据".into(), friends: Vec::new() }
        }
    };

    let friends = data.friend_list.into_iter().map(|f| Friend { name: f.name }).collect();

    LoginOutcome { ok: true, message: format!("已登录: {}", data.name), friends }
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
        let outcome = do_login("127.0.0.1:10086", "ssss", "555").await;
        assert!(outcome.ok, "expected success, got: {}", outcome.message);
        // ssss (uid 4) has no friends in the seed DB, so the list may be empty;
        // the important thing is the full gate->TCP->1006 round-trip succeeded.
    }
}
