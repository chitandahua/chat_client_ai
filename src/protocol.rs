// Wire protocol — Seam 1. Pure, I/O-free: frame encode/decode + typed messages.
//
// Frame: [id: u32 BE][body_len: u16 BE][body: JSON bytes]. Max body 1024.
// Message ids (server-side): 1005 LoginRequest, 1006 LoginResponse.

use serde::{Deserialize, Serialize};

pub const LOGIN_REQUEST: u32 = 1005;
pub const LOGIN_RESPONSE: u32 = 1006;

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
    pub name: String,
    #[serde(default)]
    pub back: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Apply {
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
}
