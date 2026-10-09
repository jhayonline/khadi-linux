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
//! - `displays <seconds left to confirm a change, or 0>` followed by one tab-separated
//!   field per monitor, left to right, each `<name> <0|1 main> <x> <y> <mode in use>
//!   <modes it offers, comma-separated>`. A mode is `<width>x<height>@<millihertz>`.
//!
//! The shell sends `activate <n>`, `close <n>`, `move <n>`, `terminal`, `split`,
//! `fullscreen`, `panel <left|right|bottom>`, `reload-input` (the input settings in the
//! configuration file changed), `displays-keep`, `displays-revert`, or
//! `displays-apply` followed by one `<name>=<mode>` per monitor, left to right, with a
//! `*` after the one that is to be the main display.

use crate::{Panel, Panels};

pub const SOCKET_ENV: &str = "EDEX_COMP_SOCKET";

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
                "\t{name} {} {} {} {} {}",
                monitor.main as u8,
                monitor.x,
                monitor.y,
                monitor.current.encode(),
                modes.join(",")
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
}

/// A message from the compositor to the shell.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    State(DesktopState),
    /// The launcher shortcut was pressed.
    Launcher,
    /// The settings shortcut was pressed.
    Settings,
    Displays(Displays),
}

impl Event {
    pub const LAUNCHER: &str = "launcher\n";
    pub const SETTINGS: &str = "settings\n";

    pub fn decode(line: &str) -> Option<Event> {
        match line.trim_end_matches('\n') {
            "launcher" => return Some(Event::Launcher),
            "settings" => return Some(Event::Settings),
            _ => {}
        }
        if line.starts_with("displays ") {
            return Displays::decode(line).map(Event::Displays);
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
            Command::ApplyDisplays(choices) => {
                let mut line = "displays-apply".to_string();
                for choice in choices {
                    let main = if choice.main { "*" } else { "" };
                    line.push_str(&format!(" {}={}{main}", choice.name, choice.mode.encode()));
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
                    let (mode, main) = match mode.strip_suffix('*') {
                        Some(mode) => (mode, true),
                        None => (mode, false),
                    };
                    Some(DisplayChoice {
                        name: name.to_string(),
                        mode: Mode::decode(mode)?,
                        main,
                    })
                })
                .collect();
            return choices.filter(|choices| !choices.is_empty()).map(Command::ApplyDisplays);
        }
        let command = match (words.next()?, words.next()) {
            ("activate", Some(index)) => Command::Activate(index.parse().ok()?),
            ("close", Some(index)) => Command::Close(index.parse().ok()?),
            ("move", Some(index)) => Command::Move(index.parse().ok()?),
            ("panel", Some("left")) => Command::TogglePanel(Panel::Left),
            ("panel", Some("right")) => Command::TogglePanel(Panel::Right),
            ("panel", Some("bottom")) => Command::TogglePanel(Panel::Bottom),
            ("terminal", None) => Command::Terminal,
            ("split", None) => Command::Split,
            ("fullscreen", None) => Command::Fullscreen,
            ("displays-keep", None) => Command::KeepDisplays,
            ("displays-revert", None) => Command::RevertDisplays,
            ("reload-input", None) => Command::ReloadInput,
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
                },
                DisplayInfo {
                    name: "HDMI-A-1".into(),
                    main: false,
                    x: 1920,
                    y: 0,
                    current: mode(1920, 1080, 60000),
                    modes: vec![mode(3840, 2160, 30000)],
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
            Command::ApplyDisplays(vec![
                DisplayChoice { name: "eDP-1".into(), mode: mode(1920, 1080, 60052), main: false },
                DisplayChoice { name: "HDMI-A-1".into(), mode: mode(3840, 2160, 30000), main: true },
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
