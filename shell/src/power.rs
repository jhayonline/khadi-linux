//! Locking by itself: before the machine sleeps, and when asked to from outside.
//!
//! The system's login service (logind) says when the machine is about to sleep, and
//! waits a moment for the lock to go up first. Applications that are showing
//! something to watch say so over D-Bus, which holds off the lock that comes with
//! being left alone; that lock itself is the compositor's doing, as it sees the input.

use crate::sysmon::Battery;
use crate::theme::Theme;
use crate::ui;
use eframe::egui::{self, Align, Align2, Key, Layout, Modifiers, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, vec2};
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, Proxy};

const LOGIN: &str = "org.freedesktop.login1";

#[derive(Default)]
struct Shared {
    /// Set when something asked for the lock: sleep coming, or `loginctl lock-session`.
    lock_wanted: bool,
    /// Held while sleep waits on the lock; dropping it lets the machine sleep.
    waiting: Option<(OwnedFd, Instant)>,
    /// Applications asking that the screen be left alone: a cookie, and who asked.
    holds: Vec<(u32, String)>,
    last_cookie: u32,
}

pub struct Power {
    shared: Arc<Mutex<Shared>>,
    session: Option<Connection>,
    checked: Instant,
    holding: bool,
}

/// Asks logind to wait for this program before sleeping.
fn delay_sleep(manager: &Proxy) -> Option<OwnedFd> {
    let fd: zbus::zvariant::OwnedFd = manager
        .call("Inhibit", &("sleep", "DEs-UI", "Locking the screen", "delay"))
        .map_err(|e| eprintln!("des-ui: cannot lock before sleep: {e}"))
        .ok()?;
    Some(fd.into())
}

fn watch_sleep(shared: Arc<Mutex<Shared>>, ctx: egui::Context, own_power_key: bool) -> zbus::Result<()> {
    let system = Connection::system()?;
    let manager = Proxy::new(&system, LOGIN, "/org/freedesktop/login1", "org.freedesktop.login1.Manager")?;

    // The power key opens the power menu, for as long as this shell is there to show
    // it; without this the system switches off at the first press.
    let _power_key: Option<zbus::zvariant::OwnedFd> = own_power_key
        .then(|| manager.call("Inhibit", &("handle-power-key", "DEs-UI", "Showing the power menu", "block")))
        .and_then(|held| held.map_err(|e| eprintln!("des-ui: the power key keeps its usual meaning: {e}")).ok());

    // `loginctl lock-session`, and whatever else locks sessions through logind.
    if let Ok(id) = std::env::var("XDG_SESSION_ID") {
        let path: zbus::Result<zbus::zvariant::OwnedObjectPath> = manager.call("GetSession", &(id.as_str(),));
        if let Ok(path) = path {
            let (system, shared, ctx) = (system.clone(), shared.clone(), ctx.clone());
            std::thread::spawn(move || {
                let Ok(session) = Proxy::new(&system, LOGIN, path, "org.freedesktop.login1.Session") else { return };
                let Ok(locks) = session.receive_signal("Lock") else { return };
                for _ in locks {
                    shared.lock().unwrap().lock_wanted = true;
                    ctx.request_repaint();
                }
            });
        }
    }

    let mut held = delay_sleep(&manager);
    for signal in manager.receive_signal("PrepareForSleep")? {
        let going: bool = signal.body().deserialize().unwrap_or(false);
        if going {
            if let Some(fd) = held.take() {
                let mut shared = shared.lock().unwrap();
                shared.lock_wanted = true;
                shared.waiting = Some((fd, Instant::now()));
            }
            ctx.request_repaint();
            // Whatever happens to the lock, sleep is not held up for long.
            let shared = shared.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(3));
                shared.lock().unwrap().waiting = None;
            });
        } else {
            // Awake again: be ready for the next time.
            shared.lock().unwrap().waiting = None;
            held = delay_sleep(&manager);
            ctx.request_repaint();
        }
    }
    Ok(())
}

/// The service applications call to keep the screen as it is while they play something.
struct ScreenSaver {
    shared: Arc<Mutex<Shared>>,
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl ScreenSaver {
    fn inhibit(&self, #[zbus(header)] header: zbus::message::Header<'_>, _application: String, _reason: String) -> u32 {
        let sender = header.sender().map(|name| name.to_string()).unwrap_or_default();
        let mut shared = self.shared.lock().unwrap();
        shared.last_cookie = shared.last_cookie.wrapping_add(1).max(1);
        let cookie = shared.last_cookie;
        shared.holds.push((cookie, sender));
        cookie
    }

    fn un_inhibit(&self, cookie: u32) {
        self.shared.lock().unwrap().holds.retain(|(held, _)| *held != cookie);
    }
}

impl Power {
    /// `own_power_key` when this shell is the desktop, and so can show the power menu.
    pub fn start(ctx: egui::Context, own_power_key: bool) -> Power {
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let for_sleep = shared.clone();
        std::thread::spawn(move || {
            if let Err(e) = watch_sleep(for_sleep, ctx, own_power_key) {
                eprintln!("des-ui: not watching for sleep: {e}");
            }
        });
        let saver = || ScreenSaver { shared: shared.clone() };
        let session = zbus::blocking::connection::Builder::session()
            .and_then(|builder| builder.name("org.freedesktop.ScreenSaver"))
            .and_then(|builder| builder.serve_at("/org/freedesktop/ScreenSaver", saver()))
            .and_then(|builder| builder.serve_at("/ScreenSaver", saver()))
            .and_then(|builder| builder.build())
            .map_err(|e| eprintln!("des-ui: applications cannot hold off the lock: {e}"))
            .ok();
        Power { shared, session, checked: Instant::now(), holding: false }
    }

    /// Whether something asked for the screen to be locked since last asked here.
    pub fn lock_wanted(&mut self) -> bool {
        std::mem::take(&mut self.shared.lock().unwrap().lock_wanted)
    }

    /// Whether the machine is waiting on the lock before it sleeps.
    pub fn sleep_waiting(&self) -> bool {
        self.shared.lock().unwrap().waiting.is_some()
    }

    /// Lets the machine sleep: the lock is up, or there is to be none.
    pub fn let_sleep(&self) {
        self.shared.lock().unwrap().waiting = None;
    }

    /// Whether an application is asking that the screen be left alone. Looked into
    /// every few seconds, dropping the requests of applications that have gone.
    pub fn holding(&mut self) -> bool {
        if self.checked.elapsed() < Duration::from_secs(5) {
            return self.holding;
        }
        self.checked = Instant::now();
        let holds = self.shared.lock().unwrap().holds.clone();
        let mut gone = Vec::new();
        if let Some(bus) = self.session.as_ref().and_then(|session| zbus::blocking::fdo::DBusProxy::new(session).ok()) {
            for (cookie, sender) in &holds {
                let alive = zbus::names::BusName::try_from(sender.as_str())
                    .ok()
                    .and_then(|name| bus.name_has_owner(name).ok())
                    .unwrap_or(false);
                if !alive {
                    gone.push(*cookie);
                }
            }
        }
        let mut shared = self.shared.lock().unwrap();
        shared.holds.retain(|(cookie, _)| !gone.contains(cookie));
        self.holding = !shared.holds.is_empty();
        self.holding
    }
}

/// What the power menu was asked to do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    Lock,
    Suspend,
    Restart,
    ShutDown,
    LogOut,
}

impl Act {
    /// Carries out what the system does; locking and logging out are the shell's own.
    pub fn run(self) {
        let verb = match self {
            Act::Suspend => "suspend",
            Act::Restart => "reboot",
            Act::ShutDown => "poweroff",
            Act::Lock | Act::LogOut => return,
        };
        if let Err(e) = std::process::Command::new("systemctl").arg(verb).spawn() {
            eprintln!("des-ui: cannot {verb}: {e}");
        }
    }
}

/// Name, what it does, the act, and whether it takes a second click to mean it.
const CHOICES: [(&str, &str, Act, bool); 5] = [
    ("LOCK", "Hide everything behind your password", Act::Lock, false),
    ("SLEEP", "Everything stays as it is, using almost no power", Act::Suspend, false),
    ("RESTART", "Closes everything and starts the machine again", Act::Restart, true),
    ("SHUT DOWN", "Closes everything and switches off", Act::ShutDown, true),
    ("LOG OUT", "Closes everything and returns to the login screen", Act::LogOut, true),
];

/// Hours and minutes, as the frame writes a duration.
pub fn duration(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} MIN"),
        (hours, 0) => format!("{hours} H"),
        (hours, minutes) => format!("{hours} H {minutes:02} MIN"),
    }
}

/// The battery in a line, for the power menu.
pub fn battery_line(battery: Battery) -> String {
    let left = battery.minutes.map(duration);
    match (battery.plugged, left) {
        (true, Some(left)) => format!("Battery at {}%, charging. Full in {}.", battery.percent, left.to_lowercase()),
        (true, None) => format!("Battery at {}%, on mains power.", battery.percent),
        (false, Some(left)) => format!("Battery at {}%. About {} left.", battery.percent, left.to_lowercase()),
        (false, None) => format!("Battery at {}%.", battery.percent),
    }
}

/// The power menu, in place of the terminal.
#[derive(Default)]
pub struct Menu {
    pub open: bool,
    /// The choice clicked once and waiting for its second click, and when.
    armed: Option<(Act, Instant)>,
}

impl Menu {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.armed = None;
    }

    pub fn show(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, battery: Option<Battery>) -> Option<Act> {
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.open = false;
            return None;
        }
        self.armed = self.armed.filter(|(_, at)| at.elapsed() < Duration::from_secs(5));
        let mut chosen = None;
        let margin = (rect.width() * 0.05).clamp(16.0, 56.0);
        let area = Rect::from_min_max(rect.min + vec2(margin, 28.0), rect.max - vec2(margin, 14.0));
        ui.allocate_new_ui(UiBuilder::new().max_rect(area), |ui| {
            ui.horizontal(|ui| {
                ui::tracked(ui, "POWER", ui::display_bold(22.0), ui::TEXT, 3.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::button(ui, theme, "DONE", Some("ESC"), false).clicked() {
                        self.open = false;
                    }
                });
            });
            ui.add_space(10.0);
            if let Some(battery) = battery {
                let low = !battery.plugged && battery.percent <= 15;
                ui::text(ui, &battery_line(battery), ui::mono(12.0), if low { ui::CAUTION } else { ui::SECONDARY });
            }
            ui.add_space(18.0);
            for (name, what, act, twice) in CHOICES {
                let armed = self.armed.is_some_and(|(waiting, _)| waiting == act);
                let (row, response) = ui.allocate_exact_size(vec2(ui.available_width().min(560.0), 58.0), Sense::click());
                let painter = ui.painter_at(row);
                let (fill, line) = match (armed, response.hovered()) {
                    (true, _) => (ui::PANEL, ui::CAUTION),
                    (false, true) => (ui::PANEL, theme.main),
                    (false, false) => (ui::GROUND, ui::LINE_STRONG),
                };
                painter.rect_filled(row, 0.0, fill);
                painter.rect_stroke(row, 0.0, Stroke::new(1.0, line), StrokeKind::Inside);
                let title = if armed { format!("CLICK AGAIN TO {name}") } else { name.to_string() };
                let ink = if armed { ui::CAUTION } else { ui::TEXT };
                painter.text(row.left_top() + vec2(18.0, 12.0), Align2::LEFT_TOP, title, ui::display_bold(14.0), ink);
                painter.text(row.left_top() + vec2(18.0, 34.0), Align2::LEFT_TOP, what, ui::mono(11.0), ui::SECONDARY);
                if response.clicked() {
                    if twice && !armed {
                        self.armed = Some((act, Instant::now()));
                    } else {
                        chosen = Some(act);
                    }
                }
                ui.add_space(8.0);
            }
        });
        if chosen.is_some() {
            self.open = false;
            self.armed = None;
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_durations_and_the_battery() {
        assert_eq!(duration(45), "45 MIN");
        assert_eq!(duration(120), "2 H");
        assert_eq!(duration(72), "1 H 12 MIN");
        let battery = |plugged, minutes| Battery { percent: 23, plugged, minutes };
        assert_eq!(battery_line(battery(false, Some(72))), "Battery at 23%. About 1 h 12 min left.");
        assert_eq!(battery_line(battery(true, Some(40))), "Battery at 23%, charging. Full in 40 min.");
        assert_eq!(battery_line(battery(true, None)), "Battery at 23%, on mains power.");
    }
}
