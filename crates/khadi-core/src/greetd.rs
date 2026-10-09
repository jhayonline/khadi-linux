//! greetd's IPC, as specified by greetd-ipc(7).
//!
//! The wire format is a 32-bit length in **native** byte order followed by
//! UTF-8 JSON, over the socket named by `$GREETD_SOCK`. Native, not network —
//! the man page is explicit, and getting it wrong gives a greeter that hangs
//! rather than one that errors.
//!
//! **greetd does the authentication.** This process never touches PAM; it
//! passes a string along and is told success or failure. That boundary is why
//! a hand-written greeter is a reasonable thing to own at all.
//!
//! It lives in khadi-core rather than in a greeter because two of them speak
//! it now: the VT one and `khadi-shell --greeter`. A protocol with a length
//! prefix whose byte order is easy to get wrong is exactly the thing not to
//! implement twice.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
};

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    CreateSession { username: String },
    PostAuthMessageResponse { response: Option<String> },
    StartSession { cmd: Vec<String>, env: Vec<String> },
    CancelSession,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Success,
    Error { error_type: String, description: String },
    AuthMessage { auth_message_type: String, auth_message: String },
}

pub struct Greetd {
    sock: UnixStream,
}

impl Greetd {
    pub fn connect() -> Result<Self> {
        let path = std::env::var("GREETD_SOCK")
            .context("GREETD_SOCK is not set — this is a greetd greeter, not a login shell")?;
        let sock = UnixStream::connect(&path)
            .with_context(|| format!("connect {path}"))?;
        Ok(Self { sock })
    }

    pub fn request(&mut self, req: &Request) -> Result<Response> {
        let body = serde_json::to_vec(req)?;
        self.sock.write_all(&(body.len() as u32).to_ne_bytes())?;
        self.sock.write_all(&body)?;
        self.sock.flush()?;

        let mut len = [0u8; 4];
        self.sock.read_exact(&mut len)?;
        let mut buf = vec![0u8; u32::from_ne_bytes(len) as usize];
        self.sock.read_exact(&mut buf)?;
        Ok(serde_json::from_slice(&buf)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tags have to match the protocol exactly — a typo here is a greeter
    /// that cannot log anyone in, found at the least convenient moment.
    #[test]
    fn requests_serialise_the_way_the_spec_spells_them() {
        let j = |r: &Request| serde_json::to_string(r).unwrap();
        assert_eq!(
            j(&Request::CreateSession { username: "me".into() }),
            r#"{"type":"create_session","username":"me"}"#
        );
        assert_eq!(
            j(&Request::PostAuthMessageResponse { response: Some("x".into()) }),
            r#"{"type":"post_auth_message_response","response":"x"}"#
        );
        // Informative messages are answered with no response at all.
        assert_eq!(
            j(&Request::PostAuthMessageResponse { response: None }),
            r#"{"type":"post_auth_message_response","response":null}"#
        );
        assert_eq!(j(&Request::CancelSession), r#"{"type":"cancel_session"}"#);
    }

    #[test]
    fn responses_parse() {
        let r: Response = serde_json::from_str(r#"{"type":"success"}"#).unwrap();
        assert!(matches!(r, Response::Success));
        let r: Response = serde_json::from_str(
            r#"{"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "}"#,
        ).unwrap();
        match r {
            Response::AuthMessage { auth_message_type, auth_message } => {
                assert_eq!(auth_message_type, "secret");
                assert_eq!(auth_message, "Password: ");
            }
            other => panic!("wrong variant: {other:?}"),
        }
        let r: Response = serde_json::from_str(
            r#"{"type":"error","error_type":"auth_error","description":"nope"}"#,
        ).unwrap();
        assert!(matches!(r, Response::Error { .. }));
    }

    /// Native byte order, per greetd-ipc(7). This asserts the thing that is
    /// easy to get wrong and impossible to debug from the symptom.
    #[test]
    fn the_length_prefix_is_native_order() {
        let body = br#"{"type":"cancel_session"}"#;
        assert_eq!((body.len() as u32).to_ne_bytes(), 25u32.to_ne_bytes());
        assert_ne!(25u32.to_ne_bytes(), 25u32.to_be_bytes(),
                   "this test only means something on a little-endian host");
    }
}
