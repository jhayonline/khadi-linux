//! Checking the password, through PAM, on a thread of its own.
//!
//! PAM blocks. `pam_unix` runs the setuid `unix_chkpwd` helper, and on a wrong
//! password it sleeps for a couple of seconds before answering — deliberately, to
//! slow guessing down. Doing that on the thread that serves Wayland would stop the
//! lock screen repainting and leave it looking crashed at exactly the moment the
//! user is watching it, so the attempt runs on a worker and the answer comes back
//! through a channel.
//!
//! No `pam_open_session` here. The session is already open; this only asks whether
//! the person at the keyboard is the one who left.

use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// The PAM service: `/etc/pam.d/khadi-lock`, installed by `install.sh`.
///
/// Not a borrowed name like `login` or `system-auth`. Those carry rules that do not
/// belong on an unlock — session setup, password changing — and on some
/// configurations they refuse to run for a non-root caller, which is how a lock
/// screen ends up unable to accept any password at all.
const SERVICE: &str = "khadi-lock";

pub enum Outcome {
    Accepted,
    /// Wrong password, or PAM refused for any other reason. The reason is logged
    /// rather than shown: telling the screen why it failed tells whoever is standing
    /// at it too.
    Rejected,
}

pub struct Checker {
    results: Receiver<Outcome>,
    sender: Sender<Outcome>,
    /// Written to when an answer is ready. The event loop waits on Wayland's socket
    /// and on this together, because a finished PAM call produces no Wayland event
    /// and without it the screen would sit on "checking" until the next keypress.
    wake: Arc<OwnedFd>,
    /// Set while a worker is out, so Enter cannot start a second one.
    busy: bool,
}

impl Checker {
    pub fn new(wake: Arc<OwnedFd>) -> Checker {
        let (sender, results) = channel();
        Checker {
            results,
            sender,
            wake,
            busy: false,
        }
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Starts an attempt. The password is moved onto the worker and dropped there.
    pub fn submit(&mut self, user: String, password: String) {
        if self.busy {
            return;
        }
        self.busy = true;
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let outcome = check(&user, &password);
            // The receiver is gone only if the lock screen is already exiting.
            let _ = sender.send(outcome);
            let _ = rustix::io::write(wake.as_fd(), b"1");
        });
    }

    /// The result of the attempt, once there is one.
    pub fn poll(&mut self) -> Option<Outcome> {
        match self.results.try_recv() {
            Ok(outcome) => {
                self.busy = false;
                Some(outcome)
            }
            Err(_) => None,
        }
    }
}

fn check(user: &str, password: &str) -> Outcome {
    let mut authenticator = match pam::Authenticator::with_password(SERVICE) {
        Ok(authenticator) => authenticator,
        Err(e) => {
            // Almost always a missing /etc/pam.d/khadi-lock. Worth being loud about:
            // without the service file nothing can ever unlock.
            eprintln!("khadi-lock: cannot start PAM service {SERVICE}: {e}");
            return Outcome::Rejected;
        }
    };
    authenticator.get_handler().set_credentials(user, password);
    // `authenticate` here is pam_authenticate followed by pam_acct_mgmt, so an
    // expired or locked account is refused as well as a wrong password.
    match authenticator.authenticate() {
        Ok(()) => Outcome::Accepted,
        Err(e) => {
            eprintln!("khadi-lock: rejected: {e}");
            Outcome::Rejected
        }
    }
}

/// Who to authenticate as: the owner of this process.
///
/// From the real user id, not from `$USER`. The two are the same in an ordinary
/// session, but the environment is not where identity should come from in the one
/// program whose job is to establish it.
pub fn current_user() -> Option<String> {
    let uid = rustix::process::getuid().as_raw();
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next();
        let Some(Ok(entry)) = fields.next().map(str::parse::<u32>) else {
            continue;
        };
        if entry == uid {
            return Some(name.to_string());
        }
    }
    None
}
