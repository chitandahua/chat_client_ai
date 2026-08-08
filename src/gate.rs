// Gate HTTP client — login, register, reset password, get verify code.
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GateLoginInfo {
    pub id: i64,
    pub user: String,
    pub token: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct VerifyCodeInfo {
    pub email: String,
    #[serde(default)]
    pub code: String,
}

#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error("gate returned status {status}: {message}")]
    GateRejected { status: i64, message: String },
    #[error("gate response missing login data")]
    MissingData,
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Deserialize)]
struct GateResponse<T> {
    pub status: i64,
    #[serde(default)]
    pub message: String,
    pub data: Option<T>,
}

/// POST to a gate endpoint and deserialize the response into a typed `GateResponse<T>`.
async fn post_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    gate_url: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<GateResponse<T>, GateError> {
    let resp = client
        .post(format!("{}{}", gate_url.trim_end_matches('/'), path))
        .json(body)
        .send()
        .await?;
    resp.json().await.map_err(GateError::Http)
}

/// Validate a gate response's status and return its data payload.
fn data<T>(parsed: GateResponse<T>) -> Result<T, GateError> {
    if parsed.status != 0 {
        return Err(GateError::GateRejected { status: parsed.status, message: parsed.message });
    }
    parsed.data.ok_or(GateError::MissingData)
}

/// Log in via the gate server's HTTP `/user_login` endpoint.
pub async fn user_login(gate_url: &str, user: &str, passwd: &str) -> Result<GateLoginInfo, GateError> {
    let client = reqwest::Client::new();
    let parsed: GateResponse<GateLoginInfo> = post_json(
        &client,
        gate_url,
        "/user_login",
        &serde_json::json!({ "user": user, "passwd": passwd }),
    )
    .await?;
    data(parsed)
}

/// Register a new account. Server requires a valid verify code for the email.
pub async fn register(
    gate_url: &str,
    user: &str,
    email: &str,
    passwd: &str,
    confirm_passwd: &str,
    verify_code: &str,
) -> Result<(), GateError> {
    let client = reqwest::Client::new();
    let parsed: GateResponse<serde_json::Value> = post_json(
        &client,
        gate_url,
        "/user_register",
        &serde_json::json!({
            "user": user, "email": email, "passwd": passwd,
            "confirm_passwd": confirm_passwd, "verify_code": verify_code,
        }),
    )
    .await?;
    data(parsed).map(|_| ())
}

/// Reset a password using an email verify code.
pub async fn reset_password(
    gate_url: &str,
    user: &str,
    email: &str,
    passwd: &str,
    verify_code: &str,
) -> Result<(), GateError> {
    let client = reqwest::Client::new();
    let parsed: GateResponse<serde_json::Value> = post_json(
        &client,
        gate_url,
        "/reset_password",
        &serde_json::json!({
            "user": user, "email": email, "passwd": passwd, "verify_code": verify_code,
        }),
    )
    .await?;
    data(parsed).map(|_| ())
}

/// Request a verification code for an email.
pub async fn get_verify_code(gate_url: &str, email: &str) -> Result<VerifyCodeInfo, GateError> {
    let client = reqwest::Client::new();
    let parsed: GateResponse<VerifyCodeInfo> = post_json(
        &client,
        gate_url,
        "/get_verify_code",
        &serde_json::json!({ "email": email }),
    )
    .await?;
    data(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    /// Serve one fake HTTP response on a loopback listener, return its URL.
    async fn mock_http_server(status: i64, message: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let body = if status == 0 {
            format!(
                r#"{{"status":0,"message":"Ok","data":{{"id":4,"user":"ssss","token":"tok","host":"127.0.0.1","port":18080}}}}"#
            )
        } else {
            format!(r#"{{"status":{status},"message":"{message}","data":null}}"#)
        };

        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(&mut sock);
            let mut line = String::new();
            let mut content_len = 0usize;
            loop {
                line.clear();
                if reader.read_line(&mut line).await.unwrap() == 0 {
                    break;
                }
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.split_once(':').map(|(_, v)| v.trim()) {
                    if v.to_lowercase().starts_with("application/json") {
                        let _ = v; // body is JSON; content-length parsed below
                    }
                }
                if let Some(v) = line.strip_prefix("Content-Length:") {
                    content_len = v.trim().parse().unwrap_or(0);
                }
            }
            // drain request body
            let mut req_body = vec![0u8; content_len];
            let _ = tokio::io::AsyncReadExt::read_exact(&mut reader, &mut req_body).await;

            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        });

        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn user_login_success_parses_credentials() {
        let url = mock_http_server(0, "Ok").await;
        let info = user_login(&url, "ssss", "555").await.unwrap();
        assert_eq!(
            info,
            GateLoginInfo { id: 4, user: "ssss".into(), token: "tok".into(), host: "127.0.0.1".into(), port: 18080 }
        );
    }

    #[tokio::test]
    async fn user_login_rejected_on_nonzero_status() {
        let url = mock_http_server(1, "password wrong").await;
        match user_login(&url, "ssss", "bad").await {
            Err(GateError::GateRejected { status, message }) => {
                assert_eq!(status, 1);
                assert_eq!(message, "password wrong");
            }
            other => panic!("expected GateRejected, got {:?}", other.map(|_| ())),
        }
    }

    #[tokio::test]
    async fn register_success_returns_ok() {
        let url = mock_http_server(0, "Ok").await;
        register(&url, "carol", "c@x.com", "pw", "pw", "code").await.unwrap();
    }

    #[tokio::test]
    async fn register_rejected_on_mismatched_verify_code() {
        let url = mock_http_server(6, "Invalid verify code").await;
        match register(&url, "carol", "c@x.com", "pw", "pw", "bad").await {
            Err(GateError::GateRejected { status, .. }) => assert_eq!(status, 6),
            other => panic!("expected rejection, got {:?}", other.map(|_| ())),
        }
    }

    #[tokio::test]
    async fn reset_password_success_returns_ok() {
        let url = mock_http_server(0, "Ok").await;
        reset_password(&url, "ssss", "s@x.com", "newpw", "code").await.unwrap();
    }
}
