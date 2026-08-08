// Wire protocol — Seam 1. Pure, I/O-free: frame encode/decode + typed messages.
//
// Frame: [id: u32 BE][body_len: u16 BE][body: JSON bytes]. Max body 1024.
// Message ids (server-side): 1005 LoginRequest, 1006 LoginResponse.

use serde::{Deserialize, Serialize};

pub const LOGIN_REQUEST: u32 = 1005;
pub const LOGIN_RESPONSE: u32 = 1006;
pub const SEARCH_REQUEST: u32 = 1007;
pub const SEARCH_RESPONSE: u32 = 1008;
pub const ADD_FRIEND_REQUEST: u32 = 1009;
pub const ADD_FRIEND_RESPONSE: u32 = 1010;
pub const NOTIFY_ADD_FRIEND: u32 = 1011;
pub const AUTH_FRIEND_REQUEST: u32 = 1013;
pub const AUTH_FRIEND_RESPONSE: u32 = 1014;
pub const NOTIFY_AUTH_FRIEND: u32 = 1015;
pub const TEXT_CHAT_REQUEST: u32 = 1017;
pub const TEXT_CHAT_RESPONSE: u32 = 1018;
pub const NOTIFY_TEXT_CHAT: u32 = 1019;

pub const PREFIX_LEN: usize = 6;
pub const MAX_BODY_LEN: usize = 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("body exceeds max length {MAX_BODY_LEN}")]
    BodyTooLong,
    #[error("frame too short to contain a header")]
    FrameTooShort,
    #[error("truncated frame: declared body length {declared} but only {available} bytes present")]
    Truncated { declared: usize, available: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: u32,
    pub body: Vec<u8>,
}

impl Frame {
    pub fn new(id: u32, body: impl Into<Vec<u8>>) -> Frame {
        Frame { id, body: body.into() }
    }
}

/// Encode a frame into wire bytes: [id: u32 BE][len: u16 BE][body].
pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, ProtocolError> {
    if frame.body.len() > MAX_BODY_LEN {
        return Err(ProtocolError::BodyTooLong);
    }
    let mut out = Vec::with_capacity(PREFIX_LEN + frame.body.len());
    out.extend_from_slice(&frame.id.to_be_bytes());
    out.extend_from_slice(&(frame.body.len() as u16).to_be_bytes());
    out.extend_from_slice(&frame.body);
    Ok(out)
}

/// Decode a single complete frame from raw bytes.
pub fn decode_frame(bytes: &[u8]) -> Result<Frame, ProtocolError> {
    if bytes.len() < PREFIX_LEN {
        return Err(ProtocolError::FrameTooShort);
    }
    let id = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
    let len = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
    if bytes.len() < PREFIX_LEN + len {
        return Err(ProtocolError::Truncated {
            declared: len,
            available: bytes.len() - PREFIX_LEN,
        });
    }
    Ok(Frame { id, body: bytes[PREFIX_LEN..PREFIX_LEN + len].to_vec() })
}

// ---- Login messages ----

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoginRequest {
    pub uid: i64,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Friend {
    /// Friend's uid. The server added this to the login friend_list.
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub back: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Apply {
    /// Applicant's uid (0 if the server didn't supply it).
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub status: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoginData {
    pub uid: i64,
    pub token: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub friend_list: Vec<Friend>,
    #[serde(default)]
    pub apply_list: Vec<Apply>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoginResponse {
    pub status: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub data: Option<LoginData>,
}

// ---- Text chat messages ----

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextChatData {
    pub msgid: i64,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextChatRequest {
    pub fromuid: i64,
    pub touid: i64,
    pub text_array: Vec<TextChatData>,
}

/// A text message delivered by the server. Because of a known server quirk
/// (same-server delivery uses 1015 instead of 1019), incoming text must be
/// parsed from BOTH notify ids — the body shape is identical.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IncomingText {
    pub fromuid: i64,
    pub touid: i64,
    pub text_array: Vec<TextChatData>,
}

// ---- Search / add friend / auth friend ----

/// 1007 search by uid OR name; at least one field is present.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchRequest {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub uid: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

impl SearchRequest {
    pub fn by_name(name: impl Into<String>) -> Self {
        Self { uid: 0, name: name.into() }
    }
}

/// 1008 search response: a user found by search. Note the server returns the
/// raw UserInfo (not wrapped), and on failure returns an error envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserInfo {
    pub id: i64,
    pub name: String,
}

/// 1008 search response: the server returns the raw `UserInfo` on success, or
/// an error envelope on failure. Model both as one type.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchResponse {
    Found(UserInfo),
    Error(SimpleResponse),
}

impl SearchResponse {
    pub fn from_body(body: &[u8]) -> Result<Self, serde_json::Error> {
        if let Ok(user) = serde_json::from_slice::<UserInfo>(body) {
            return Ok(SearchResponse::Found(user));
        }
        serde_json::from_slice::<SimpleResponse>(body).map(SearchResponse::Error)
    }
}

/// 1009 add-friend request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AddFriendRequest {
    pub uid: i64,
    pub touid: i64,
}

/// 1011 friend-apply push delivered to the target user.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotifyAddFriend {
    pub applyuid: i64,
    pub name: String,
}

/// 1013 auth-friend request (approve/deny). `fromuid` = the person approving.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthFriendRequest {
    pub fromuid: i64,
    pub touid: i64,
}

/// Generic `{"error":0,"message":"..."}` response shared by add-friend (1010)
/// and auth-friend (1014).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimpleResponse {
    #[serde(default)]
    pub error: i64,
    #[serde(default)]
    pub message: String,
}

impl SimpleResponse {
    pub fn is_ok(&self) -> bool {
        self.error == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_login_request_matches_known_wire_bytes() {
        let req = LoginRequest { uid: 4, token: "992ebc4c-25c7-4c3e-a79d-584d3cd9c9ab".into() };
        let body = serde_json::to_vec(&req).unwrap();
        let wire = encode_frame(&Frame::new(LOGIN_REQUEST, body)).unwrap();

        assert_eq!(&wire[0..4], &1005u32.to_be_bytes());
        assert_eq!(&wire[4..6], &[0x00, 0x38]); // body length 56 BE
        let decoded_req: LoginRequest = serde_json::from_slice(&wire[6..]).unwrap();
        assert_eq!(decoded_req, req);
    }

    #[test]
    fn decode_login_response_from_known_json() {
        let json = r#"{"data":{"apply_list":[],"friend_list":[{"name":"aaa","back":""}],"name":"ssss","token":"992ebc4c-25c7-4c3e-a79d-584d3cd9c9ab","uid":4},"message":"","status":0}"#;
        let body = json.as_bytes();
        let frame = decode_frame(&encode_frame(&Frame::new(LOGIN_RESPONSE, body)).unwrap()).unwrap();
        assert_eq!(frame.id, LOGIN_RESPONSE);

        let resp: LoginResponse = serde_json::from_slice(&frame.body).unwrap();
        assert_eq!(resp.status, 0);
        let data = resp.data.expect("login data");
        assert_eq!(data.uid, 4);
        assert_eq!(data.name, "ssss");
        assert_eq!(data.friend_list.len(), 1);
        assert_eq!(data.friend_list[0].name, "aaa");
    }

    #[test]
    fn frame_roundtrip_preserves_id_and_body() {
        let frame = Frame::new(1017, br#"{"fromuid":4,"touid":1,"text_array":[{"msgid":1,"content":"hi"}]}"#.to_vec());
        let wire = encode_frame(&frame).unwrap();
        let decoded = decode_frame(&wire).unwrap();
        assert_eq!(decoded, frame);
    }

    #[test]
    fn decode_rejects_oversized_and_truncated_frames() {
        assert_eq!(
            encode_frame(&Frame::new(1005, vec![0u8; MAX_BODY_LEN + 1])),
            Err(ProtocolError::BodyTooLong)
        );
        assert_eq!(decode_frame(&[0u8; 4]), Err(ProtocolError::FrameTooShort));

        let frame = Frame::new(1005, b"{}".to_vec());
        let wire = encode_frame(&frame).unwrap();
        assert_eq!(
            decode_frame(&wire[..wire.len() - 1]),
            Err(ProtocolError::Truncated { declared: 2, available: 1 })
        );
    }

    #[test]
    fn text_chat_request_serializes_with_msgid_content() {
        let req = TextChatRequest {
            fromuid: 4,
            touid: 1,
            text_array: vec![TextChatData { msgid: 1, content: "hi".into() }],
        };
        let wire = encode_frame(&Frame::new(TEXT_CHAT_REQUEST, serde_json::to_vec(&req).unwrap())).unwrap();
        assert_eq!(&wire[0..4], &1017u32.to_be_bytes());

        let parsed: TextChatRequest = serde_json::from_slice(&wire[6..]).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn incoming_text_parses_from_both_1015_and_1019() {
        let body = br#"{"fromuid":1,"touid":4,"text_array":[{"msgid":1,"content":"hi"}]}"#.to_vec();
        for id in [NOTIFY_TEXT_CHAT, NOTIFY_AUTH_FRIEND] {
            let wire = encode_frame(&Frame::new(id, body.clone())).unwrap();
            let frame = decode_frame(&wire).unwrap();
            let parsed: IncomingText = serde_json::from_slice(&frame.body).unwrap();
            assert_eq!(parsed.fromuid, 1);
            assert_eq!(parsed.touid, 4);
            assert_eq!(parsed.text_array[0].content, "hi");
        }
    }

    #[test]
    fn search_request_serializes_by_name() {
        let by_name = serde_json::to_value(SearchRequest::by_name("aaa")).unwrap();
        assert_eq!(by_name, serde_json::json!({"name": "aaa"}));
    }

    #[test]
    fn add_friend_request_serializes_uid_and_touid() {
        let req = AddFriendRequest { uid: 4, touid: 1 };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json, serde_json::json!({"uid": 4, "touid": 1}));
    }

    #[test]
    fn auth_friend_request_serializes_from_and_to() {
        let req = AuthFriendRequest { fromuid: 4, touid: 1 };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json, serde_json::json!({"fromuid": 4, "touid": 1}));
    }

    #[test]
    fn notify_add_friend_parses_push() {
        let body = br#"{"applyuid":3,"name":"bbb"}"#.to_vec();
        let frame = decode_frame(&encode_frame(&Frame::new(NOTIFY_ADD_FRIEND, body)).unwrap()).unwrap();
        let parsed: NotifyAddFriend = serde_json::from_slice(&frame.body).unwrap();
        assert_eq!(parsed.applyuid, 3);
        assert_eq!(parsed.name, "bbb");
    }

    #[test]
    fn simple_response_reports_ok() {
        let ok: SimpleResponse = serde_json::from_str(r#"{"error":0,"message":"Success"}"#).unwrap();
        assert!(ok.is_ok());
        let err: SimpleResponse = serde_json::from_str(r#"{"error":1011,"message":"UserNotFound"}"#).unwrap();
        assert!(!err.is_ok());
    }

    #[test]
    fn search_response_parses_found_user() {
        let resp = SearchResponse::from_body(br#"{"id":1,"name":"aaa","email":"x"}"#).unwrap();
        match resp {
            SearchResponse::Found(u) => {
                assert_eq!(u.id, 1);
                assert_eq!(u.name, "aaa");
            }
            SearchResponse::Error(_) => panic!("expected found"),
        }
    }

    #[test]
    fn search_response_parses_error_envelope() {
        let resp = SearchResponse::from_body(br#"{"error":1011,"message":"UserNotFound"}"#).unwrap();
        match resp {
            SearchResponse::Found(_) => panic!("expected error"),
            SearchResponse::Error(e) => assert_eq!(e.error, 1011),
        }
    }
}
