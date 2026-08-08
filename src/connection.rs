// Async TCP connection to the chat server — Seam 3.
// Owns the socket; sends typed frames and reads framed responses.
use crate::protocol::{decode_frame, encode_frame, Frame, ProtocolError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("connection closed while reading frame")]
    Eof,
}

pub struct Connection {
    stream: TcpStream,
}

impl Connection {
    /// Open a TCP connection to the chat server.
    pub async fn connect(host: &str, port: u16) -> Result<Self, ConnectionError> {
        let stream = TcpStream::connect((host, port)).await?;
        Ok(Self { stream })
    }

    /// Send one typed frame.
    pub async fn send(&mut self, frame: &Frame) -> Result<(), ConnectionError> {
        let wire = encode_frame(frame)?;
        self.stream.write_all(&wire).await?;
        Ok(())
    }

    /// Read one complete frame. Returns the raw frame; the caller decodes the body.
    pub async fn recv(&mut self) -> Result<Frame, ConnectionError> {
        let mut header = [0u8; 6];
        self.read_exact(&mut header).await?;
        let len = u16::from_be_bytes([header[4], header[5]]) as usize;
        let mut buf = vec![0u8; 6 + len];
        buf[..6].copy_from_slice(&header);
        self.read_exact(&mut buf[6..]).await?;
        Ok(decode_frame(&buf)?)
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ConnectionError> {
        self.stream.read_exact(buf).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ConnectionError::Eof
            } else {
                ConnectionError::Io(e)
            }
        })?;
        Ok(())
    }

    /// Convenience: send a login request and read the response frame.
    pub async fn login(&mut self, uid: i64, token: &str) -> Result<Frame, ConnectionError> {
        let body = serde_json::to_vec(&crate::protocol::LoginRequest {
            uid,
            token: token.to_string(),
        })
        .unwrap();
        self.send(&Frame::new(crate::protocol::LOGIN_REQUEST, body)).await?;
        self.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{LOGIN_RESPONSE, LoginResponse};
    use serde_json::json;
    use tokio::net::TcpListener;

    /// In-process mock chat server: accepts one connection, reads a frame,
    /// replies with a 1006 login response, then replies with a text push.
    async fn mock_chat_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            // read request frame
            let mut header = [0u8; 6];
            let _ = sock.read_exact(&mut header).await;
            let len = u16::from_be_bytes([header[4], header[5]]) as usize;
            let mut body = vec![0u8; len];
            let _ = sock.read_exact(&mut body).await;

            // respond 1006
            let login_rsp = json!({
                "data": {"apply_list": [], "friend_list": [{"name":"aaa","back":""}], "name":"ssss", "token":"tok", "uid":4},
                "message": "",
                "status": 0
            })
            .to_string();
            let rsp = Frame::new(LOGIN_RESPONSE, login_rsp.into_bytes());
            let wire = encode_frame(&rsp).unwrap();
            let _ = sock.write_all(&wire).await;
        });

        port
    }

    #[tokio::test]
    async fn login_roundtrip_against_mock_server() {
        let port = mock_chat_server().await;
        let mut conn = Connection::connect("127.0.0.1", port).await.unwrap();
        let frame = conn.login(4, "tok").await.unwrap();

        assert_eq!(frame.id, LOGIN_RESPONSE);
        let resp: LoginResponse = serde_json::from_slice(&frame.body).unwrap();
        assert_eq!(resp.status, 0);
        let data = resp.data.expect("login data");
        assert_eq!(data.uid, 4);
        assert_eq!(data.friend_list.len(), 1);
    }

    #[tokio::test]
    async fn connect_to_closed_port_fails() {
        // bind then drop to free the port; connect should fail with io error
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let err = Connection::connect("127.0.0.1", port).await.err().unwrap();
        assert!(matches!(err, ConnectionError::Io(_)));
    }
}
