//! The lock screen: the session stays, hidden behind a password.
//!
//! The compositor does the hiding: once told to lock, it shows nothing but the shell
//! and sends every key to it. The shell draws this screen over all of its own, checks
//! the password with the system's helper for that, and tells the compositor to unlock.

use crate::greeter::{Identity, load_picture};
use crate::theme::Theme;
use crate::ui;
use eframe::egui::{self, Align2, CentralPanel, Event, Frame, Key, TextureHandle, pos2, vec2};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// The PAM service whose rules decide an unlock: `/etc/pam.d/khadi-lock`,
/// installed by `install.sh`.
const SERVICE: &str = "khadi-lock";

/// Whether passwords can be checked here. Without that, locking would be a trap.
///
/// The service file, not the helper behind it. `pam_unix` reaches the shadow file
/// through the setuid `unix_chkpwd`, but it is only one of the things that may
/// answer: a machine set up for a fingerprint reader or a smart card has those in
/// the same file and no use for the helper at all.
pub fn can_check() -> bool {
    std::path::Path::new("/etc/pam.d").join(SERVICE).exists()
}

/// Checks `user`'s password. An error means the check could not be made at all.
///
/// Through PAM rather than by running `unix_chkpwd` directly, which is what this
/// did before. The helper only compares a local password, so going straight to it
/// steps around everything else the machine has been told to do: `pam_faillock`,
/// which locks an account after repeated failures, account expiry, and any other
/// way of proving who you are. A lock screen that cannot be rate-limited is a lock
/// screen you can sit and guess at.
pub fn check(user: &str, password: &str) -> Result<bool, String> {
    let mut authenticator = pam::Authenticator::with_password(SERVICE)
        .map_err(|e| format!("The password could not be checked: {e}"))?;
    authenticator.get_handler().set_credentials(user, password);
    // pam_authenticate followed by pam_acct_mgmt, so an expired or locked account
    // is refused as well as a wrong password.
    match authenticator.authenticate() {
        Ok(()) => Ok(true),
        // A refusal is not an error: it is the answer. Only the reason is dropped,
        // because what the screen may say and what the log may say are different —
        // whoever is standing at it has not proved who they are yet.
        Err(e) => {
            eprintln!("khadi: unlock refused: {e}");
            Ok(false)
        }
    }
}

/// Who to authenticate as: the owner of this process, from the real user id rather
/// than `$USER`. The two match in an ordinary session, but the environment is not
/// where identity should come from on the one screen whose job is to establish it.
pub fn current_user() -> Option<String> {
    let uid = unsafe { libc_getuid() };
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

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

enum State {
    Ready,
    Checking,
    Denied(String),
}

pub struct Lock {
    pub locked: bool,
    user: String,
    password: String,
    state: State,
    result: Option<Receiver<Result<bool, String>>>,
    /// The user's picture, once looked for.
    picture: Option<Option<TextureHandle>>,
}

impl Lock {
    pub fn new() -> Lock {
        Lock {
            locked: false,
            user: current_user()
                .or_else(|| std::env::var("USER").ok())
                .unwrap_or_else(|| "user".to_string()),
            password: String::new(),
            state: State::Ready,
            result: None,
            picture: None,
        }
    }

    /// The user's picture changed: look for it again when next shown.
    pub fn forget_picture(&mut self) {
        self.picture = None;
    }

    pub fn engage(&mut self) {
        self.locked = true;
        self.password.clear();
        self.state = State::Ready;
    }

    /// Unlocks without a password: for when the compositor says the lock is off.
    pub fn release(&mut self) {
        self.locked = false;
        self.password.clear();
        self.result = None;
    }

    fn submit(&mut self, ctx: &egui::Context) {
        if self.password.is_empty() || matches!(self.state, State::Checking) {
            return;
        }
        self.state = State::Checking;
        let (tx, rx) = channel();
        self.result = Some(rx);
        let password = std::mem::take(&mut self.password);
        let user = self.user.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let outcome = check(&user, &password);
            // A wrong guess costs time, so that guessing is slow.
            if outcome != Ok(true) {
                std::thread::sleep(Duration::from_millis(1500));
            }
            let _ = tx.send(outcome);
            ctx.request_repaint();
        });
    }

    /// Takes the keyboard and draws the lock screen over everything. Returns true on
    /// the frame the right password is given.
    ///
    /// `below` draws whatever else belongs on the screen (what is playing), given the
    /// part of it inside the margins.
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, below: impl FnOnce(&mut egui::Ui, egui::Rect)) -> bool {
        if let Some(outcome) = self.result.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.result = None;
            match outcome {
                Ok(true) => {
                    self.release();
                    return true;
                }
                Ok(false) => self.state = State::Denied("That password did not match".to_string()),
                Err(why) => self.state = State::Denied(why),
            }
        }

        for event in ctx.input(|i| i.events.clone()) {
            if matches!(self.state, State::Checking) {
                break;
            }
            match event {
                Event::Text(text) | Event::Paste(text) => {
                    self.password.extend(text.chars().filter(|c| !c.is_control()));
                    self.state = State::Ready;
                }
                Event::Key { key, pressed: true, modifiers, .. } => match key {
                    Key::Backspace if modifiers.ctrl => self.password.clear(),
                    Key::Backspace => {
                        self.password.pop();
                    }
                    Key::Escape => self.password.clear(),
                    Key::Enter => self.submit(ctx),
                    _ => {}
                },
                _ => {}
            }
        }

        if self.picture.is_none() {
            self.picture = Some(load_picture(ctx, &self.user));
        }
        let (edge, message, ink) = match &self.state {
            State::Ready => (theme.main, String::new(), ui::SECONDARY),
            State::Checking => (ui::SECONDARY, "Checking…".to_string(), ui::SECONDARY),
            State::Denied(why) => (ui::CAUTION, why.clone(), ui::CAUTION),
        };
        CentralPanel::default().frame(Frame::new().fill(ui::GROUND)).show(ctx, |ui| {
            let screen = ui.max_rect();
            ui.painter().rect_filled(screen, 0.0, ui::GROUND);
            let inner = screen.shrink2(vec2(64.0, 48.0));
            let brand = ui::galley(ui, "Khadi", ui::display_bold(14.0), ui::SECONDARY, 4.0);
            let brand_width = brand.size().x;
            ui.painter().galley(inner.left_top(), brand, ui::SECONDARY);
            let locked = ui::galley(ui, "LOCKED", ui::display(14.0), edge, 4.0);
            ui.painter().galley(inner.left_top() + vec2(brand_width + 14.0, 0.0), locked, edge);

            let now = chrono::Local::now();
            let date = ui::galley(ui, &now.format("%a %d %b").to_string().to_uppercase(), ui::display(13.0), ui::SECONDARY, 2.0);
            let date_width = date.size().x;
            ui.painter().galley(pos2(inner.right() - date_width, inner.top() + 12.0), date, ui::SECONDARY);
            ui.painter().text(
                pos2(inner.right() - date_width - 10.0, inner.top() + 30.0),
                Align2::RIGHT_BOTTOM,
                now.format("%H:%M").to_string(),
                ui::display(28.0),
                ui::TEXT,
            );

            Identity {
                user: &self.user,
                picture: self.picture.as_ref().and_then(|picture| picture.as_ref()),
                edge,
                message: &message,
                message_ink: ink,
                typed: self.password.chars().count(),
                busy: matches!(self.state, State::Checking),
                hint: "ENTER to unlock",
            }
            .draw(ui, theme);

            ui.painter().text(
                inner.left_bottom(),
                Align2::LEFT_BOTTOM,
                "Your session is still running behind this screen.",
                ui::mono(12.0),
                ui::LABEL,
            );
            below(ui, inner);
        });
        // The clock and the cursor keep moving.
        ctx.request_repaint_after(Duration::from_millis(250));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrong_password_is_refused() {
        if !can_check() {
            return;
        }
        let user = std::env::var("USER").unwrap();
        assert_eq!(check(&user, "surely-not-this-one-7f3a"), Ok(false));
        assert_eq!(check(&user, ""), Ok(false));
    }
}
