//! The line protocol between the compositor and the shell, spoken over a Unix socket
//! whose path the compositor passes to the shell in [`SOCKET_ENV`].
//!
//! The compositor sends events, one per line:
//! - `state <displays> <open panels, as bits> <0|1 terminal shares the workspace>
//!   <application with the keyboard, or "-" for the terminal>` followed by one
//!   tab-separated field per application, each `<display> <place> <title>`, whenever
//!   any of that changes. A place is `0` hidden, `1` shown, `2` shown in the right half;
//! - `launcher` when the user asks for the application launcher;
//! - `settings` when the user asks for the settings;
//! - `music` when the user asks for the music player;
//! - `notifications` when the user asks for the list of notifications;
//! - `power` when the user asks for the power menu;
//! - `lock` when the user asks for the screen to be locked, which the shell answers
//!   with its own `lock` once it is ready to show the lock screen;
//! - `locked <0|1>` whenever the screen becomes locked or unlocked;
//! - `displays <seconds left to confirm a change, or 0>` followed by one tab-separated
//!   field per monitor, left to right, each `<name> <0|1 main> <x> <y> <mode in use>
//!   <modes it offers, comma-separated>`. A mode is `<width>x<height>@<millihertz>`.
//!
//! The shell sends `activate <n>`, `close <n>`, `move <n>`, `terminal`, `split`,
//! `toast <x y w h|none>`, `fullscreen`, `lock`, `unlock`, `panel <left|right|bottom>`, `reload-input` (the input settings in the
//! configuration file changed), `displays-keep`, `displays-revert`, or
//! `displays-apply` followed by one `<name>=<mode>` per monitor, left to right, with a
//! `*` after the one that is to be the main display, then `/<scale>:<turn>:<use>`: the
//! scale in percent, the turn in degrees, and `o` for a display of its own, `m` for one
//! mirroring the main display, `x` for one turned off.

use crate::{Panel, Panels};

pub const SOCKET_ENV: &str = "DES_COMP_SOCKET";

/// Where an application is on its display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    Hidden,
    /// Filling the workspace, or its left half when it is split.
    Main,
    /// In the right half of a split workspace.
    Side,
}

/// One open application.
#[derive(Debug, Clone, PartialEq)]
pub struct AppInfo {
    pub title: String,
    /// Index of the display the application lives on; 0 is the one with the frame.
    pub screen: usize,
    pub place: Place,
}

/// The displays and open applications, as the compositor reports them.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopState {
    pub screens: usize,
    pub panels: Panels,
    /// Whether the terminal has only the left half of the workspace, an application
    /// being in the right half.
    pub terminal_split: bool,
    /// The application with the keyboard; `None` when the shell has it.
    pub focus: Option<usize>,
    pub apps: Vec<AppInfo>,
}

impl Default for DesktopState {
    fn default() -> Self {
        DesktopState {
            screens: 1,
            panels: Panels::default(),
            terminal_split: false,
            focus: None,
            apps: Vec::new(),
        }
    }
}

impl DesktopState {
    /// The application covering the terminal on the main display, if any.
    pub fn covering(&self) -> Option<usize> {
        self.apps
            .iter()
            .position(|app| app.screen == 0 && app.place == Place::Main)
    }

    pub fn encode(&self) -> String {
        let focus = match self.focus {
            Some(index) => index.to_string(),
            None => "-".to_string(),
        };
        let mut line = format!(
            "state {} {} {} {focus}",
            self.screens,
            self.panels.bits(),
            self.terminal_split as u8
        );
        for app in &self.apps {
            let place = match app.place {
                Place::Hidden => 0,
                Place::Main => 1,
                Place::Side => 2,
            };
            line.push_str(&format!("\t{} {place} ", app.screen));
            // Titles come from applications and must not break the framing.
            line.extend(app.title.chars().map(|c| if c.is_control() { ' ' } else { c }));
        }
        line.push('\n');
        line
    }

    pub fn decode(line: &str) -> Option<DesktopState> {
        let mut fields = line.trim_end_matches('\n').split('\t');
        let mut head = fields.next()?.strip_prefix("state ")?.split(' ');
        let screens: usize = head.next()?.parse().ok()?;
        let panels = Panels::from_bits(head.next()?.parse().ok()?)?;
        let terminal_split = match head.next()? {
            "0" => false,
            "1" => true,
            _ => return None,
        };
        let focus = match head.next()? {
            "-" => None,
            index => Some(index.parse().ok()?),
        };
        if screens == 0 || head.next().is_some() {
            return None;
        }
        let mut apps = Vec::new();
        for field in fields {
            let mut parts = field.splitn(3, ' ');
            let screen: usize = parts.next()?.parse().ok()?;
            let place = match parts.next()? {
                "0" => Place::Hidden,
                "1" => Place::Main,
                "2" => Place::Side,
                _ => return None,
            };
            if screen >= screens {
                return None;
            }
            apps.push(AppInfo {
                title: parts.next()?.to_string(),
                screen,
                place,
            });
        }
        if focus.is_some_and(|index| index >= apps.len()) {
            return None;
        }
        Some(DesktopState {
            screens,
            panels,
            terminal_split,
            focus,
            apps,
        })
    }
}

/// A resolution and refresh rate a monitor can run at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    /// Refresh rate in millihertz, as monitors report it: 59940 for 59.94 Hz.
    pub refresh: u32,
}

impl Mode {
    pub fn encode(&self) -> String {
        format!("{}x{}@{}", self.width, self.height, self.refresh)
    }

    pub fn decode(text: &str) -> Option<Mode> {
        let (size, refresh) = text.split_once('@')?;
        let (width, height) = size.split_once('x')?;
        let mode = Mode {
            width: width.parse().ok()?,
            height: height.parse().ok()?,
            refresh: refresh.parse().ok()?,
        };
        (mode.width > 0 && mode.height > 0 && mode.refresh > 0).then_some(mode)
    }
}

/// What a monitor is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Usage {
    /// A display of its own, beside the others.
    #[default]
    Own,
    /// Showing what the main display shows.
    Mirror,
    Off,
}

/// How a monitor is set beyond its mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    /// How large things are drawn, in percent: 100 is one to one.
    pub scale: u32,
    /// Quarter turns clockwise, in degrees: 0, 90, 180 or 270.
    pub turn: u16,
    pub usage: Usage,
}

impl Default for Setup {
    fn default() -> Self {
        Setup { scale: 100, turn: 0, usage: Usage::Own }
    }
}

impl Setup {
    /// The scales on offer.
    pub const SCALES: [u32; 5] = [100, 125, 150, 175, 200];

    pub fn encode(&self) -> String {
        let usage = match self.usage {
            Usage::Own => 'o',
            Usage::Mirror => 'm',
            Usage::Off => 'x',
        };
        format!("{}:{}:{usage}", self.scale, self.turn)
    }

    pub fn decode(text: &str) -> Option<Setup> {
        let mut parts = text.split(':');
        let scale: u32 = parts.next()?.parse().ok()?;
        let turn: u16 = parts.next()?.parse().ok()?;
        let usage = match parts.next()? {
            "o" => Usage::Own,
            "m" => Usage::Mirror,
            "x" => Usage::Off,
            _ => return None,
        };
        (parts.next().is_none() && (50..=400).contains(&scale) && matches!(turn, 0 | 90 | 180 | 270)).then_some(Setup { scale, turn, usage })
    }

    /// The size a mode comes to on the desktop: turned, then scaled.
    pub fn logical(&self, mode: Mode) -> (i32, i32) {
        let (width, height) = if self.turn % 180 == 90 { (mode.height, mode.width) } else { (mode.width, mode.height) };
        let scaled = |side: u32| ((side as f64 * 100.0 / self.scale.max(1) as f64).round() as i32).max(1);
        (scaled(width), scaled(height))
    }
}

/// One connected monitor, as the compositor is driving it.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    /// The connector's name, such as `HDMI-A-1`.
    pub name: String,
    /// Whether it is the display with the frame.
    pub main: bool,
    pub x: i32,
    pub y: i32,
    pub current: Mode,
    pub modes: Vec<Mode>,
    pub setup: Setup,
}

/// The connected monitors, left to right.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Displays {
    /// Seconds left to confirm a change before it is undone; 0 when none is waiting.
    pub pending: u32,
    pub monitors: Vec<DisplayInfo>,
}

impl Displays {
    pub fn encode(&self) -> String {
        let mut line = format!("displays {}", self.pending);
        for monitor in &self.monitors {
            let modes: Vec<String> = monitor.modes.iter().map(Mode::encode).collect();
            let name: String = monitor.name.chars().filter(|c| !c.is_whitespace()).collect();
            line.push_str(&format!(
                "\t{name} {} {} {} {} {} {}",
                monitor.main as u8,
                monitor.x,
                monitor.y,
                monitor.current.encode(),
                modes.join(","),
                monitor.setup.encode()
            ));
        }
        line.push('\n');
        line
    }

    pub fn decode(line: &str) -> Option<Displays> {
        let mut fields = line.trim_end_matches('\n').split('\t');
        let pending = fields.next()?.strip_prefix("displays ")?.parse().ok()?;
        let mut monitors = Vec::new();
        for field in fields {
            let mut parts = field.split(' ');
            let name = parts.next().filter(|name| !name.is_empty())?.to_string();
            let main = match parts.next()? {
                "0" => false,
                "1" => true,
                _ => return None,
            };
            let x = parts.next()?.parse().ok()?;
            let y = parts.next()?.parse().ok()?;
            let current = Mode::decode(parts.next()?)?;
            let modes: Option<Vec<Mode>> = parts.next()?.split(',').map(Mode::decode).collect();
            // A compositor from before monitors could be turned or scaled says nothing more.
            let setup = match parts.next() {
                Some(setup) => Setup::decode(setup)?,
                None => Setup::default(),
            };
            if parts.next().is_some() {
                return None;
            }
            monitors.push(DisplayInfo {
                name,
                main,
                x,
                y,
                current,
                modes: modes?,
                setup,
            });
        }
        Some(Displays { pending, monitors })
    }
}

/// What the user wants one monitor to do.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayChoice {
    pub name: String,
    pub mode: Mode,
    pub main: bool,
    pub setup: Setup,
}

/// What a screenshot is of.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShotMode {
    /// The display in use.
    Screen,
    /// The window in use.
    Window,
    /// A piece of the display, which the user marks out in the shell.
    Region,
    /// Whichever of those the user then picks, in the shell.
    Pick,
}

impl ShotMode {
    pub fn word(self) -> &'static str {
        match self {
            ShotMode::Screen => "screen",
            ShotMode::Window => "window",
            ShotMode::Region => "region",
            ShotMode::Pick => "pick",
        }
    }

    pub fn from_word(word: &str) -> Option<ShotMode> {
        Some(match word {
            "screen" => ShotMode::Screen,
            "window" => ShotMode::Window,
            "region" => ShotMode::Region,
            "pick" => ShotMode::Pick,
            _ => return None,
        })
    }
}

/// How a recording stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Recording {
    On,
    Off,
    /// It could not be started.
    Failed,
}

/// A message from the compositor to the shell.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    State(DesktopState),
    /// The launcher shortcut was pressed.
    Launcher,
    /// The settings shortcut was pressed.
    Settings,
    /// The music shortcut was pressed.
    Music,
    /// The notifications shortcut was pressed.
    Notifications,
    /// The power key or shortcut was pressed.
    Power,
    /// A screenshot was taken: its width and height, and the file of plain pixels
    /// (four bytes each, rows from the top) that holds it. `window` is where in it
    /// the window in use lies (left, top, width, height), when the shell may need to
    /// cut that out.
    Shot { mode: ShotMode, width: u32, height: u32, window: Option<(u32, u32, u32, u32)>, path: String },
    Recording(Recording),
    /// The lock shortcut was pressed.
    LockRequested,
    /// The screen is now locked, or no longer.
    Locked(bool),
    Displays(Displays),
}

impl Event {
    pub const LAUNCHER: &str = "launcher\n";
    pub const SETTINGS: &str = "settings\n";
    pub const MUSIC: &str = "music\n";
    pub const NOTIFICATIONS: &str = "notifications\n";
    pub const POWER: &str = "power\n";

    pub fn shot(mode: ShotMode, width: i32, height: i32, window: Option<(i32, i32, i32, i32)>, path: &str) -> String {
        let window = window.map_or("-".to_string(), |(x, y, w, h)| format!("{x},{y},{w},{h}"));
        format!("shot {} {width} {height} {window} {path}\n", mode.word())
    }

    /// `state` is `on`, `off` or `failed`.
    pub fn recording(state: &str) -> String {
        format!("recording {state}\n")
    }
    pub const LOCK_REQUESTED: &str = "lock\n";

    pub fn locked(locked: bool) -> String {
        format!("locked {}\n", locked as u8)
    }

    pub fn decode(line: &str) -> Option<Event> {
        match line.trim_end_matches('\n') {
            "launcher" => return Some(Event::Launcher),
            "settings" => return Some(Event::Settings),
            "music" => return Some(Event::Music),
            "notifications" => return Some(Event::Notifications),
            "power" => return Some(Event::Power),
            "recording on" => return Some(Event::Recording(Recording::On)),
            "recording off" => return Some(Event::Recording(Recording::Off)),
            "recording failed" => return Some(Event::Recording(Recording::Failed)),
            "lock" => return Some(Event::LockRequested),
            "locked 0" => return Some(Event::Locked(false)),
            "locked 1" => return Some(Event::Locked(true)),
            _ => {}
        }
        if line.starts_with("displays ") {
            return Displays::decode(line).map(Event::Displays);
        }
        if let Some(shot) = line.trim_end_matches('\n').strip_prefix("shot ") {
            // The path comes last and may hold spaces.
            let mut parts = shot.splitn(5, ' ');
            let mode = ShotMode::from_word(parts.next()?)?;
            let width = parts.next()?.parse().ok()?;
            let height = parts.next()?.parse().ok()?;
            let window = match parts.next()? {
                "-" => None,
                sides => {
                    let mut sides = sides.split(',').map(|side| side.parse::<u32>().ok());
                    let window = (sides.next()??, sides.next()??, sides.next()??, sides.next()??);
                    sides.next().is_none().then_some(window)
                }
            };
            let path = parts.next().filter(|path| !path.is_empty())?.to_string();
            return Some(Event::Shot { mode, width, height, window, path });
        }
        DesktopState::decode(line).map(Event::State)
    }
}

/// A request from the shell to the compositor.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Show the application at this index, on whichever display it lives.
    Activate(usize),
    /// Ask the application at this index to close.
    Close(usize),
    /// Move the application at this index to the next display.
    Move(usize),
    /// Uncover the terminal and give it the keyboard.
    Terminal,
    /// Split the workspace in use between two windows, or undo that.
    Split,
    /// Let the applications on the display in use cover all of it, or undo that.
    Fullscreen,
    /// Open or collapse one of the frame's panels.
    TogglePanel(Panel),
    /// Drive the monitors this way, listed left to right. The change is undone
    /// unless [`Command::KeepDisplays`] follows in time.
    ApplyDisplays(Vec<DisplayChoice>),
    KeepDisplays,
    RevertDisplays,
    /// The keyboard, mouse or touchpad settings were changed: read them again.
    ReloadInput,
    /// Hide everything but the shell, which is showing the lock screen.
    Lock,
    /// The password was given: bring everything back.
    Unlock,
    /// The shell is showing notifications in this part of its window (x, y, width,
    /// height), which is to be seen above any application; or is showing none.
    Toast(Option<(i32, i32, i32, i32)>),
    /// An application is showing something to watch (or no longer): do not lock the
    /// screen for want of input meanwhile.
    HoldIdle(bool),
    /// Take a screenshot.
    Shot(ShotMode),
    /// Record the display in use into this file.
    Record(String),
    RecordStop,
}

impl Command {
    pub fn encode(&self) -> String {
        match self {
            Command::Activate(index) => format!("activate {index}\n"),
            Command::Close(index) => format!("close {index}\n"),
            Command::Move(index) => format!("move {index}\n"),
            Command::Terminal => "terminal\n".to_string(),
            Command::Split => "split\n".to_string(),
            Command::Fullscreen => "fullscreen\n".to_string(),
            Command::TogglePanel(Panel::Left) => "panel left\n".to_string(),
            Command::TogglePanel(Panel::Right) => "panel right\n".to_string(),
            Command::TogglePanel(Panel::Bottom) => "panel bottom\n".to_string(),
            Command::KeepDisplays => "displays-keep\n".to_string(),
            Command::RevertDisplays => "displays-revert\n".to_string(),
            Command::ReloadInput => "reload-input\n".to_string(),
            Command::Lock => "lock\n".to_string(),
            Command::Unlock => "unlock\n".to_string(),
            Command::Toast(None) => "toast none\n".to_string(),
            Command::HoldIdle(true) => "hold-idle on\n".to_string(),
            Command::HoldIdle(false) => "hold-idle off\n".to_string(),
            Command::Shot(mode) => format!("shot {}\n", mode.word()),
            Command::Record(path) => format!("record {path}\n"),
            Command::RecordStop => "record-stop\n".to_string(),
            Command::Toast(Some((x, y, w, h))) => format!("toast {x} {y} {w} {h}\n"),
            Command::ApplyDisplays(choices) => {
                let mut line = "displays-apply".to_string();
                for choice in choices {
                    let main = if choice.main { "*" } else { "" };
                    line.push_str(&format!(" {}={}{main}/{}", choice.name, choice.mode.encode(), choice.setup.encode()));
                }
                line.push('\n');
                line
            }
        }
    }

    pub fn decode(line: &str) -> Option<Command> {
        let mut words = line.split_whitespace();
        if line.split_whitespace().next() == Some("displays-apply") {
            let choices: Option<Vec<DisplayChoice>> = words
                .skip(1)
                .map(|word| {
                    let (name, mode) = word.split_once('=')?;
                    let (mode, setup) = match mode.split_once('/') {
                        Some((mode, setup)) => (mode, Setup::decode(setup)?),
                        None => (mode, Setup::default()),
                    };
                    let (mode, main) = match mode.strip_suffix('*') {
                        Some(mode) => (mode, true),
                        None => (mode, false),
                    };
                    Some(DisplayChoice {
                        name: name.to_string(),
                        mode: Mode::decode(mode)?,
                        main,
                        setup,
                    })
                })
                .collect();
            return choices.filter(|choices| !choices.is_empty()).map(Command::ApplyDisplays);
        }
        if let Some(path) = line.trim_end_matches('\n').strip_prefix("record ") {
            return (!path.is_empty()).then(|| Command::Record(path.to_string()));
        }
        if let Some(area) = line.trim_end().strip_prefix("toast ") {
            if area == "none" {
                return Some(Command::Toast(None));
            }
            let numbers: Vec<i32> = area.split(' ').map(str::parse).collect::<Result<_, _>>().ok()?;
            let [x, y, w, h] = numbers[..] else { return None };
            return Some(Command::Toast(Some((x, y, w, h))));
        }
        let command = match (words.next()?, words.next()) {
            ("activate", Some(index)) => Command::Activate(index.parse().ok()?),
            ("close", Some(index)) => Command::Close(index.parse().ok()?),
            ("move", Some(index)) => Command::Move(index.parse().ok()?),
            ("shot", Some(word)) => Command::Shot(ShotMode::from_word(word)?),
            ("record-stop", None) => Command::RecordStop,
            ("hold-idle", Some("on")) => Command::HoldIdle(true),
            ("hold-idle", Some("off")) => Command::HoldIdle(false),
            ("panel", Some("left")) => Command::TogglePanel(Panel::Left),
            ("panel", Some("right")) => Command::TogglePanel(Panel::Right),
            ("panel", Some("bottom")) => Command::TogglePanel(Panel::Bottom),
            ("terminal", None) => Command::Terminal,
            ("split", None) => Command::Split,
            ("fullscreen", None) => Command::Fullscreen,
            ("displays-keep", None) => Command::KeepDisplays,
            ("displays-revert", None) => Command::RevertDisplays,
            ("reload-input", None) => Command::ReloadInput,
            ("lock", None) => Command::Lock,
            ("unlock", None) => Command::Unlock,
            _ => return None,
        };
        words.next().is_none().then_some(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(title: &str, screen: usize, place: Place) -> AppInfo {
        AppInfo {
            title: title.into(),
            screen,
            place,
        }
    }

    #[test]
    fn state_round_trips() {
        for state in [
            DesktopState::default(),
            DesktopState {
                screens: 2,
                panels: Panels::from_bits(5).unwrap(),
                terminal_split: true,
                focus: Some(1),
                apps: vec![
                    app("Text Editor", 0, Place::Side),
                    app("Files — Home", 1, Place::Main),
                    app("x", 0, Place::Hidden),
                ],
            },
        ] {
            assert_eq!(DesktopState::decode(&state.encode()), Some(state.clone()));
            assert_eq!(Event::decode(&state.encode()), Some(Event::State(state)));
        }
    }

    #[test]
    fn covering_is_the_main_app_on_the_main_display() {
        let state = DesktopState {
            screens: 2,
            apps: vec![
                app("a", 1, Place::Main),
                app("b", 0, Place::Side),
                app("c", 0, Place::Main),
            ],
            ..Default::default()
        };
        assert_eq!(state.covering(), Some(2));
        assert_eq!(DesktopState::default().covering(), None);
    }

    #[test]
    fn titles_cannot_break_framing() {
        let state = DesktopState {
            apps: vec![app("evil\ttitle\nstate 5", 0, Place::Main)],
            ..Default::default()
        };
        let line = state.encode();
        assert_eq!(line.matches('\n').count(), 1);
        assert_eq!(DesktopState::decode(&line).unwrap().apps[0].title, "evil title state 5");
    }

    #[test]
    fn rejects_malformed_state() {
        for line in [
            "hello",
            "state x 7 0 -",
            "state 0 7 0 -",
            "state 1 9 0 -",
            "state 1 7 2 -",
            "state 1 7 0",
            "state 1 7 0 - extra",
            "state 1 7 0 0",
            "state 1 7 0 -\t1 1 on-a-missing-display",
            "state 1 7 0 -\t0 3 bad-place",
            "state 1 7 0 -\t0 1",
        ] {
            assert_eq!(DesktopState::decode(line), None, "{line}");
        }
    }

    #[test]
    fn displays_round_trip() {
        let mode = |width, height, refresh| Mode { width, height, refresh };
        let displays = Displays {
            pending: 12,
            monitors: vec![
                DisplayInfo {
                    name: "eDP-1".into(),
                    main: true,
                    x: 0,
                    y: 0,
                    current: mode(1920, 1080, 60052),
                    modes: vec![mode(1920, 1080, 60052), mode(1280, 720, 60000)],
                    setup: Setup::default(),
                },
                DisplayInfo {
                    name: "HDMI-A-1".into(),
                    main: false,
                    x: 1920,
                    y: 0,
                    current: mode(1920, 1080, 60000),
                    modes: vec![mode(3840, 2160, 30000)],
                    setup: Setup { scale: 150, turn: 90, usage: Usage::Mirror },
                },
            ],
        };
        assert_eq!(Displays::decode(&displays.encode()), Some(displays.clone()));
        assert_eq!(Event::decode(&displays.encode()), Some(Event::Displays(displays)));
        assert_eq!(Displays::decode("displays 0\n"), Some(Displays::default()));
        for line in ["displays", "displays x", "displays 0\teDP-1 1 0 0 1920x1080", "displays 0\teDP-1 2 0 0 1x1@1 1x1@1", "displays 0\teDP-1 1 0 0 0x0@0 1x1@1"] {
            assert_eq!(Displays::decode(line), None, "{line}");
        }
    }

    #[test]
    fn events_and_commands_round_trip() {
        assert_eq!(Event::decode(Event::LAUNCHER), Some(Event::Launcher));
        assert_eq!(Event::decode(Event::SETTINGS), Some(Event::Settings));
        assert_eq!(Event::decode(Event::MUSIC), Some(Event::Music));
        assert_eq!(Event::decode(Event::NOTIFICATIONS), Some(Event::Notifications));
        assert_eq!(Event::decode(Event::POWER), Some(Event::Power));
        assert_eq!(Event::decode(&Event::recording("failed")), Some(Event::Recording(Recording::Failed)));
        assert_eq!(
            Event::decode(&Event::shot(ShotMode::Region, 1920, 1080, None, "/run/user/1000/a shot.rgba")),
            Some(Event::Shot { mode: ShotMode::Region, width: 1920, height: 1080, window: None, path: "/run/user/1000/a shot.rgba".into() })
        );
        assert_eq!(
            Event::decode(&Event::shot(ShotMode::Pick, 1920, 1080, Some((300, 40, 1066, 701)), "/run/a.rgba")),
            Some(Event::Shot { mode: ShotMode::Pick, width: 1920, height: 1080, window: Some((300, 40, 1066, 701)), path: "/run/a.rgba".into() })
        );
        assert_eq!(Event::decode("shot region 1920 1080 - "), None);
        assert_eq!(Event::decode("shot pick 1920 1080 1,2,3 /run/a.rgba"), None);
        assert_eq!(Event::decode(Event::LOCK_REQUESTED), Some(Event::LockRequested));
        assert_eq!(Event::decode(&Event::locked(true)), Some(Event::Locked(true)));
        assert_eq!(Event::decode(&Event::locked(false)), Some(Event::Locked(false)));
        assert_eq!(Event::decode("locked 2"), None);
        for command in [
            Command::Activate(3),
            Command::Close(0),
            Command::Move(2),
            Command::Terminal,
            Command::Split,
            Command::Fullscreen,
            Command::TogglePanel(Panel::Left),
            Command::TogglePanel(Panel::Right),
            Command::TogglePanel(Panel::Bottom),
        ] {
            assert_eq!(Command::decode(command.encode().trim_end()), Some(command));
        }
        let mode = |width, height, refresh| Mode { width, height, refresh };
        for command in [
            Command::KeepDisplays,
            Command::RevertDisplays,
            Command::ReloadInput,
            Command::Lock,
            Command::Unlock,
            Command::Toast(None),
            Command::HoldIdle(true),
            Command::HoldIdle(false),
            Command::Shot(ShotMode::Window),
            Command::Record("/home/ada/Videos/a film.mp4".into()),
            Command::RecordStop,
            Command::Toast(Some((1500, 52, 380, 240))),
            Command::ApplyDisplays(vec![
                DisplayChoice { name: "eDP-1".into(), mode: mode(1920, 1080, 60052), main: false, setup: Setup::default() },
                DisplayChoice { name: "HDMI-A-1".into(), mode: mode(3840, 2160, 30000), main: true, setup: Setup { scale: 200, turn: 270, usage: Usage::Off } },
            ]),
        ] {
            assert_eq!(Command::decode(command.encode().trim_end()), Some(command));
        }
        for line in ["displays-apply", "displays-apply eDP-1", "displays-apply eDP-1=big", "displays-keep 1"] {
            assert_eq!(Command::decode(line), None, "{line}");
        }
        for line in ["activate", "activate -1", "terminal 1", "close 1 2", "panel top", "split 1"] {
            assert_eq!(Command::decode(line), None, "{line}");
        }
    }
}
