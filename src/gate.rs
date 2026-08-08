// Gate HTTP client — POST /user_login → {id, user, token, host, port}.
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct GateLoginInfo {
    pub id: i64,
    pub user: String,
    pub token: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
struct GateResponse {
    pub status: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub data: Option<GateLoginInfo>,
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

/// Log in via the gate server's HTTP `/user_login` endpoint.
pub async fn user_login(gate_url: &str, user: &str, passwd: &str) -> Result<GateLoginInfo, GateError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/user_login", gate_url.trim_end_matches('/')))
        .json(&serde_json::json!({ "user": user, "passwd": passwd }))
        .send()
        .await?;

    let parsed: GateResponse = resp.json().await?;
    if parsed.status != 0 {
        return Err(GateError::GateRejected { status: parsed.status, message: parsed.message });
    }
    parsed.data.ok_or(GateError::MissingData)
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
}
