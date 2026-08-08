//! The deep network client: one typed method per protocol request, hiding
//! frame encode/decode, request/response correlation, push filtering, and
//! timeouts behind a small typed interface. Talks to the socket only through
//! `NetConnection`, so the dispatch core is unit-testable with a mock.

use crate::connection::NetConnection;
use crate::protocol::{
    AddFriendRequest, AuthFriendRequest, Frame, SearchRequest, SimpleResponse, TextChatData,
    TextChatRequest, TextChatResponse, UserInfo,
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
            .request(crate::protocol::SEARCH_REQUEST, crate::protocol::SEARCH_RESPONSE, &req, on_push)
            .await?;
        match crate::protocol::SearchResponse::from_body(&frame.body) {
            Ok(crate::protocol::SearchResponse::Found(user)) => Ok(user),
            Ok(crate::protocol::SearchResponse::Error(e)) if e.error != 0 => {
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
            .request(crate::protocol::TEXT_CHAT_REQUEST, crate::protocol::TEXT_CHAT_RESPONSE, &req, on_push)
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
            .request(crate::protocol::ADD_FRIEND_REQUEST, crate::protocol::ADD_FRIEND_RESPONSE, &req, on_push)
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
            .request(crate::protocol::AUTH_FRIEND_REQUEST, crate::protocol::AUTH_FRIEND_RESPONSE, &req, on_push)
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
        let mut conn = MockConnection::new(vec![frame(crate::protocol::SEARCH_RESPONSE, &user)]);
        let mut client = Client::new(&mut conn, 4);

        let result = client.search("aaa", &mut |_| {}).await.unwrap();
        assert_eq!(result.id, 1);
        assert_eq!(result.name, "aaa");
        assert_eq!(conn.sent[0].id, crate::protocol::SEARCH_REQUEST);
    }

    #[tokio::test]
    async fn client_search_surfaces_server_error() {
        let err = r#"{"error":1011,"message":"UserNotFound"}"#.to_string();
        let mut conn = MockConnection::new(vec![frame(crate::protocol::SEARCH_RESPONSE, &err)]);
        let mut client = Client::new(&mut conn, 4);

        let result = client.search("nope", &mut |_| {}).await;
        assert!(matches!(result, Err(SearchError::Unavailable(_))));
    }

    #[tokio::test]
    async fn client_send_text_succeeds_on_ok_response() {
        let ok = r#"{"error":0,"message":""}"#.to_string();
        let mut conn = MockConnection::new(vec![frame(crate::protocol::TEXT_CHAT_RESPONSE, &ok)]);
        let mut client = Client::new(&mut conn, 4);

        client.send_text(1, "hi", &mut |_| {}).await.unwrap();
        assert_eq!(conn.sent[0].id, crate::protocol::TEXT_CHAT_REQUEST);
    }

    #[tokio::test]
    async fn client_add_friend_and_approve_roundtrip() {
        let ok = r#"{"error":0,"message":"Success"}"#.to_string();
        let mut conn = MockConnection::new(vec![
            frame(crate::protocol::ADD_FRIEND_RESPONSE, &ok),
            frame(crate::protocol::AUTH_FRIEND_RESPONSE, &ok),
        ]);
        let mut client = Client::new(&mut conn, 4);

        client.add_friend(3, &mut |_| {}).await.unwrap();
        client.approve(1, &mut |_| {}).await.unwrap();
        assert_eq!(conn.sent[0].id, crate::protocol::ADD_FRIEND_REQUEST);
        assert_eq!(conn.sent[1].id, crate::protocol::AUTH_FRIEND_REQUEST);
    }

    #[tokio::test]
    async fn client_times_out_when_no_response() {
        let mut conn = MockConnection::new(vec![]);
        let mut client = Client::new(&mut conn, 4);
        client.timeout = std::time::Duration::from_millis(20);

        let result = client.search("aaa", &mut |_| {}).await;
        assert!(matches!(result, Err(SearchError::Timeout)));
    }
}
