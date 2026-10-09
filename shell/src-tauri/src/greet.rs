//! `khadi-shell --greeter` — the login screen's half of greetd's IPC.
//!
//! THE COMMAND SET HERE IS DELIBERATELY TINY, and that is a security property
//! rather than a convenience. The greeter runs BEFORE anyone has authenticated,
//! as the unprivileged `greeter` user, and the desktop's command set includes
//! `pty_spawn`. Registering the normal handler in greeter mode would put a
//! shell one IPC call away from a login screen — a pre-auth shell as `greeter`
//! — so greeter mode gets its own Tauri builder with its own, much shorter,
//! list. The desktop's commands are not merely unused here; they are not
//! registered, and the capability manifest for this window grants nothing
//! beyond `core:default`.
//!
//! greetd does the authentication. Nothing here touches PAM: it passes a
//! string along and is told success or failure.

use std::sync::Mutex;

use khadi_core::greetd::{Greetd, Request, Response};
use serde::Serialize;
use tauri::State;

#[derive(Default)]
pub struct Session(pub Mutex<Option<Greetd>>);

/// What the frontend is told after every exchange. One shape for all of them,
/// because the greeter is a state machine with exactly three outcomes and a
/// discriminated union across an IPC boundary is harder to get wrong than
/// three commands that each return something different.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// greetd wants something typed. `secret` decides whether it is masked.
    Prompt { message: String, secret: bool },
    /// Authenticated. The caller may now start the session.
    Ready,
    /// Rejected, or the protocol failed. The greeter starts over.
    Failed { message: String },
}

fn step(r: Response) -> Step {
    match r {
        Response::Success => Step::Ready,
        Response::Error { description, .. } => Step::Failed { message: description },
        Response::AuthMessage { auth_message_type, auth_message } => match auth_message_type.as_str() {
            // greetd-ipc(7): secret, visible, info, error. Only the first two
            // are questions; the others are things to show and move past.
            "secret" => Step::Prompt { message: auth_message, secret: true },
            "visible" => Step::Prompt { message: auth_message, secret: false },
            "error" => Step::Failed { message: auth_message },
            _ => Step::Prompt { message: auth_message, secret: false },
        },
    }
}

fn with<F>(s: &Session, f: F) -> Result<Step, String>
where
    F: FnOnce(&mut Greetd) -> anyhow::Result<Response>,
{
    let mut guard = s.0.lock().map_err(|_| "greetd session poisoned".to_string())?;
    let g = guard.as_mut().ok_or("no greetd session")?;
    match f(g) {
        Ok(r) => Ok(step(r)),
        Err(e) => Ok(Step::Failed { message: e.to_string() }),
    }
}

#[tauri::command]
pub fn greet_begin(s: State<'_, Session>, username: String) -> Result<Step, String> {
    // A fresh connection per attempt. greetd closes the session on a failed
    // authentication and the protocol has no way to reuse one, so reconnecting
    // is the documented path rather than a reset.
    let g = Greetd::connect().map_err(|e| e.to_string())?;
    *s.0.lock().map_err(|_| "greetd session poisoned".to_string())? = Some(g);
    with(&s, |g| g.request(&Request::CreateSession { username }))
}

#[tauri::command]
pub fn greet_answer(s: State<'_, Session>, answer: String) -> Result<Step, String> {
    with(&s, |g| {
        g.request(&Request::PostAuthMessageResponse { response: Some(answer) })
    })
}

#[tauri::command]
pub fn greet_start(s: State<'_, Session>, cmd: Vec<String>) -> Result<Step, String> {
    with(&s, |g| g.request(&Request::StartSession { cmd, env: vec![] }))
}

#[tauri::command]
pub fn greet_cancel(s: State<'_, Session>) -> Result<(), String> {
    let mut guard = s.0.lock().map_err(|_| "greetd session poisoned".to_string())?;
    if let Some(g) = guard.as_mut() {
        let _ = g.request(&Request::CancelSession);
    }
    *guard = None;
    Ok(())
}

/// Which machine this is. One file read, and the only thing the greeter is
/// told about the host — a login screen is read by someone unverified, so the
/// same rule the lock screen follows applies here with more force.
#[tauri::command]
pub fn greet_host() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// The session greetd should start once authentication succeeds.
///
/// `--cmd` on the command line, as the VT greeter took it, so the session is
/// named in /etc/greetd/config.toml and not compiled in.
#[tauri::command]
pub fn greet_session_cmd() -> Vec<String> {
    let mut args = std::env::args().skip_while(|a| a != "--cmd");
    args.next();
    match args.next() {
        Some(c) => vec![c],
        None => vec!["start-hyprland".into()],
    }
}
