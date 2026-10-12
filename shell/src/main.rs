//! khadi: a Rust rewrite of the eDEX-UI sci-fi terminal desktop.

mod apps;
mod capture;
mod config;
mod desktop;
mod geo;
mod globe;
mod greeter;
mod launcher;
mod lock;
mod music;
mod notify;
mod panels;
mod power;
mod bootsplash;
mod radio;
mod sound;
mod settings;
mod sysmon;
mod term;
mod theme;
mod ui;

use eframe::egui::{
    self, Align, CentralPanel, Event, EventFilter, Frame, Key, Layout, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, UiBuilder, ViewportBuilder, ViewportCommand, Visuals, pos2, vec2,
};
use desktop::Desktop;
use khadi_common::{
    Panels,
    ipc::{Command, DesktopState, Place},
};
use ui::Icon;
use launcher::Launcher;
use panels::FsView;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use sysmon::SysMon;
use term::TermTab;
use theme::Theme;

const MAX_TABS: usize = 9;

const USAGE: &str = "\
Usage: khadi [OPTIONS]

Options:
  --theme <NAME|FILE>  Theme to use: its accent and terminal colours (default: signal)
  --font <FILE>        Font for prompt symbols (default: an installed Nerd Font, if any)
  --windowed           Run in a window instead of fullscreen
  --launcher           Start with the application launcher open
  --split-terminal[=below]
                       Start with the terminal split in two
  --settings[=SECTION] Start with the settings open, optionally on a section
  --music              Start with the music player open
  --power              Start with the power menu open
                       (appearance, terminal, frame, displays, network, bluetooth,
                       keyboard, mouse, globe, sound, session)
  --list-apps          Print the applications the launcher would offer, and exit
  --unlock             Unlock a locked screen from outside it, such as from a text
                       console you have signed in on
  --greeter            Run as the login screen (under greetd; alone, as a preview)
  --set-avatar <FILE>  Use this picture on the login screen
  --write-boot-splash <DIR>
                       Write the boot splash's pictures, in your theme's colours
                       (install-boot-splash.sh uses this)
  --set-location <LAT,LON[,NAME]>
                       Mark this place as yours on the globe, instead of the one
                       guessed from your address; 'auto' goes back to guessing
  --geo <on|off>       Whether to look up where addresses are, which tells an online
                       service the public addresses this machine connects to
  --screenshot <FILE>  Save a PPM screenshot after 3 seconds and exit (for testing)
  -h, --help           Show this help";

struct App {
    ctx: egui::Context,
    theme: Theme,
    shell: String,
    tabs: Vec<TermTab>,
    /// The tab in view; with the terminal split, the one on the left or on top.
    active: usize,
    /// The tab in the second pane of a split terminal.
    side: Option<usize>,
    /// Whether that second pane has the keyboard.
    side_focused: bool,
    /// Whether the panes are one above the other rather than side by side.
    stacked: bool,
    mon: SysMon,
    fs: FsView,
    /// The compositor connection; `None` when running as an ordinary window.
    desktop: Option<Desktop>,
    launcher: Launcher,
    settings: settings::Settings,
    music: music::Music,
    notices: notify::Center,
    /// Sleep, and what holds off the lock.
    power: power::Power,
    power_menu: power::Menu,
    /// Screenshots and recordings.
    capture: capture::Capture,
    /// Whether the compositor was last told that something is being watched.
    idle_held: bool,
    /// What the desktop itself gives notice of, as last seen.
    watch: notify::Watch,
    /// Where the compositor was last told the toasts are.
    toast_area: Option<Option<(i32, i32, i32, i32)>>,
    lock: lock::Lock,
    /// Whether the compositor has confirmed the lock now showing.
    lock_confirmed: bool,
    config: config::Config,
    /// The keyboard, mouse and touchpad settings, as last saved.
    input: khadi_common::input::InputSettings,
    /// The font that supplies prompt symbols, kept for when fonts are set up again.
    symbols: Option<PathBuf>,
    /// Whether the saved panel choice has been sent to the compositor yet.
    panels_synced: bool,
    /// Places for the globe, kept current in the background.
    geo: geo::Geo,
    /// Which panels are open when there is no compositor to say.
    local_panels: Panels,
    screenshot: Option<PathBuf>,
    started: Instant,
    screenshot_requested: bool,
}

impl App {
    fn new(
        ctx: egui::Context,
        theme: Theme,
        config: config::Config,
        symbols: Option<PathBuf>,
        screenshot: Option<PathBuf>,
    ) -> anyhow::Result<App> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let first = TermTab::spawn(&shell, &home, ctx.clone(), config.scrollback)?;
        let desktop = Desktop::connect(ctx.clone());
        let music = music::Music::new(ctx.clone());
        let capture = capture::Capture::new(ctx.clone());
        let power = power::Power::start(ctx.clone(), desktop.is_some());
        let mut notices = notify::Center::new(ctx.clone(), config.do_not_disturb, true);
        notices.sounds = config.notification_sounds;
        Ok(App {
            ctx,
            theme,
            shell,
            tabs: vec![first],
            active: 0,
            side: None,
            side_focused: false,
            stacked: false,
            mon: SysMon::new(),
            fs: FsView::new(),
            desktop,
            launcher: Launcher::default(),
            settings: settings::Settings::default(),
            music,
            notices,
            power,
            power_menu: power::Menu::default(),
            capture,
            idle_held: false,
            watch: notify::Watch::default(),
            toast_area: None,
            lock: lock::Lock::new(),
            lock_confirmed: false,
            geo: geo::Geo::start(config.lookups, config.location.clone()),
            local_panels: config.panels,
            config,
            input: khadi_common::input::InputSettings::load(),
            symbols,
            panels_synced: false,
            screenshot,
            started: Instant::now(),
            screenshot_requested: false,
        })
    }

    /// Opens a new tab in the active tab's working directory.
    fn new_tab(&mut self) {
        if self.tabs.len() >= MAX_TABS {
            return;
        }
        let cwd = self
            .tabs
            .get(self.focus_tab())
            .and_then(|t| t.cwd())
            .unwrap_or_else(|| PathBuf::from("/"));
        match TermTab::spawn(&self.shell, &cwd, self.ctx.clone(), self.config.scrollback) {
            Ok(tab) => {
                self.tabs.push(tab);
                self.show_tab(self.tabs.len() - 1);
            }
            Err(e) => eprintln!("khadi: cannot open tab: {e}"),
        }
    }

    /// The tab that has the keyboard.
    fn focus_tab(&self) -> usize {
        match self.side {
            Some(side) if self.side_focused => side,
            _ => self.active,
        }
    }

    /// Brings a tab into view: in the pane it is already in, or else the pane in use.
    fn show_tab(&mut self, index: usize) {
        if self.side == Some(index) {
            self.side_focused = true;
        } else if index == self.active {
            self.side_focused = false;
        } else if self.side_focused {
            self.side = Some(index);
        } else {
            self.active = index;
        }
    }

    /// Splits the terminal in two, side by side or one above the other, with a new
    /// tab in the second pane; or, when it is split, makes it whole again around the
    /// pane in use. Both tabs stay open either way.
    fn split_terminal(&mut self, stacked: bool) {
        if let Some(side) = self.side.take() {
            if self.side_focused {
                self.active = side;
            }
            self.side_focused = false;
            return;
        }
        let before = self.tabs.len();
        self.side_focused = false;
        let left = self.active;
        self.new_tab();
        // Out of tabs: put one that is already open in the second pane instead.
        let second = if self.tabs.len() > before {
            Some(self.tabs.len() - 1)
        } else {
            (0..self.tabs.len()).find(|index| *index != left)
        };
        self.active = left;
        if let Some(second) = second {
            self.side = Some(second);
            self.side_focused = true;
            self.stacked = stacked;
        }
    }

    /// Opens or closes the settings, which like the launcher take the terminal's place.
    fn toggle_settings(&mut self) {
        if self.settings.open {
            self.settings.open = false;
            self.music.open = false;
            self.notices.open = false;
            self.power_menu.open = false;
            return;
        }
        self.launcher.open = false;
        self.music.open = false;
        self.notices.open = false;
        self.power_menu.open = false;
        self.settings.open(&self.config);
        if let Some(desktop) = &mut self.desktop {
            desktop.send(Command::Terminal);
        }
    }

    /// Opens or closes the music player, which also takes the terminal's place.
    fn toggle_music(&mut self) {
        self.launcher.open = false;
        self.settings.open = false;
        self.notices.open = false;
        self.power_menu.open = false;
        self.music.toggle();
        if self.music.open {
            self.send(Command::Terminal);
        }
    }

    /// Opens or closes the list of notifications, which also takes the terminal's place.
    fn toggle_notices(&mut self) {
        self.launcher.open = false;
        self.settings.open = false;
        self.music.open = false;
        self.notices.toggle();
        if self.notices.open {
            self.send(Command::Terminal);
        }
    }

    fn toggle_recording(&mut self) {
        if let Some(command) = self.capture.toggle_recording(&mut self.notices) {
            self.send(command);
        }
    }

    /// Opens or closes the power menu, which also takes the terminal's place.
    fn toggle_power(&mut self) {
        self.launcher.open = false;
        self.settings.open = false;
        self.music.open = false;
        self.notices.open = false;
        self.power_menu.toggle();
        if self.power_menu.open {
            self.send(Command::Terminal);
        }
    }

    /// Brings into view the application a notification came from, if it has a window.
    fn follow(&mut self, chosen: notify::Chosen) {
        let name = chosen.app.to_lowercase();
        let index = self.desktop.as_ref().and_then(|desktop| {
            desktop.state.apps.iter().position(|app| !name.is_empty() && app.title.to_lowercase().contains(&name))
        });
        if let Some(index) = index {
            self.notices.open = false;
            self.power_menu.open = false;
            self.send(Command::Activate(index));
        }
    }

    /// Locks the screen, if a password can be checked to unlock it again.
    fn lock_now(&mut self) {
        if !lock::can_check() {
            eprintln!("khadi: not locking: this system has no way to check the password");
            return;
        }
        self.launcher.open = false;
        self.settings.open = false;
        self.music.open = false;
        self.notices.open = false;
        self.power_menu.open = false;
        self.lock.engage();
        self.lock_confirmed = false;
        self.send(Command::Lock);
    }

    fn send(&mut self, command: Command) {
        if let Some(desktop) = &mut self.desktop {
            desktop.send(command);
        }
    }

    /// Carries out and saves what the user changed in the settings.
    fn apply(&mut self, ctx: &egui::Context, change: settings::Change) {
        use settings::Change;
        let save = |key: &str, value: Option<&str>| {
            if let Err(e) = config::set(key, value) {
                eprintln!("khadi: cannot save the setting: {e}");
            }
        };
        match change {
            Change::Theme(name) => {
                let theme = Theme::load(&name);
                apply_style(ctx, &theme);
                self.theme = theme;
                save("theme", Some(&name));
                self.config.theme = name;
            }
            Change::FontSize(size) => {
                self.config.font_size = size;
                save("font_size", Some(&size.to_string()));
            }
            Change::GlobeRotation(on) => {
                self.config.globe_rotation = on;
                save("globe_rotation", (!on).then_some("off"));
            }
            Change::Lookups(on) => {
                self.config.lookups = on;
                save("geo", (!on).then_some("off"));
                self.geo = geo::Geo::start(on, self.config.location.clone());
            }
            Change::Location(place) => {
                let value = place.as_ref().map(|p| format!("{}, {}, {}", p.lat, p.lon, p.label));
                save("location", value.as_deref());
                self.config.location = place;
                self.geo = geo::Geo::start(self.config.lookups, self.config.location.clone());
            }
            Change::Panel(panel, open) => {
                let mut panels = self.config.panels;
                match panel {
                    khadi_common::Panel::Left => panels.left = open,
                    khadi_common::Panel::Right => panels.right = open,
                    khadi_common::Panel::Bottom => panels.bottom = open,
                }
                self.config.panels = panels;
                save("panels", Some(&config::Config::panels_value(panels)));
                // The frame follows at once; with a compositor, on its next report.
                self.local_panels = panels;
                self.panels_synced = false;
            }
            Change::Set(key, value) => {
                save(key, value.as_deref());
                // Read everything back, so that what is shown is what was saved.
                let theme = std::mem::take(&mut self.config.theme);
                let font = self.config.terminal_font.clone();
                self.config = config::Config::load();
                self.config.theme = theme;
                self.notices.sounds = self.config.notification_sounds;
                self.input = khadi_common::input::InputSettings::load();
                if self.config.terminal_font != font {
                    ui::install_fonts(ctx, self.symbols.as_deref(), self.config.terminal_font.as_deref());
                }
                // The compositor owns the devices these are about.
                if ["keyboard_", "mouse_", "touchpad_", "lock_after", "screen_off_after"].iter().any(|prefix| key.starts_with(prefix)) {
                    self.send(Command::ReloadInput);
                }
            }
            Change::Volume(percent) => {
                let level = format!("{:.2}", percent as f32 / 100.0);
                spawn_quietly("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &level]);
            }
            Change::Brightness(percent) => set_brightness(percent),
            Change::ApplyDisplays(choices) => self.send(Command::ApplyDisplays(choices)),
            Change::KeepDisplays => self.send(Command::KeepDisplays),
            Change::RevertDisplays => self.send(Command::RevertDisplays),
            Change::Lock => self.lock_now(),
            Change::Avatar(picture) => {
                let outcome = match &picture {
                    Some(path) => greeter::set_avatar(path).map(|_| ()),
                    None => greeter::remove_avatar(),
                };
                match outcome {
                    Ok(()) => {
                        self.lock.forget_picture();
                        self.settings.forget_picture();
                    }
                    Err(why) => self.notices.post("Picture not changed", &why, true),
                }
            }
            Change::LogOut => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    /// Opens or closes the launcher. It is drawn where the terminal is, so anything
    /// covering that must make way.
    fn toggle_launcher(&mut self) {
        self.settings.open = false;
        self.music.open = false;
        self.notices.open = false;
        self.power_menu.open = false;
        self.launcher.toggle();
        if self.launcher.open {
            if let Some(desktop) = &mut self.desktop {
                desktop.send(Command::Terminal);
            }
        }
    }

    /// Runs a terminal application in a tab of its own.
    fn run_in_terminal(&mut self, entry: &apps::AppEntry) {
        // When no more tabs can be opened, this types into the current one.
        self.new_tab();
        let words: Vec<String> = entry.exec.iter().map(|word| panels::shell_quote(word)).collect();
        let line = format!("{}\n", words.join(" "));
        let tab = self.focus_tab();
        self.tabs[tab].write(line.as_bytes());
    }

    /// Implements `--screenshot`: captures one frame, writes it as a PPM, then quits.
    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = &self.screenshot else {
            return;
        };
        if !self.screenshot_requested && self.started.elapsed() > Duration::from_secs(3) {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let mut data = format!("P6\n{} {}\n255\n", image.size[0], image.size[1]).into_bytes();
            for pixel in &image.pixels {
                data.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
            }
            if let Err(e) = std::fs::write(path, data) {
                eprintln!("khadi: cannot write {}: {e}", path.display());
            }
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Handles app-level shortcuts and returns the events left over for the terminal.
    fn take_shortcuts(&mut self, ctx: &egui::Context) -> Vec<Event> {
        let mut rest = Vec::new();
        for event in ctx.input(|i| i.events.clone()) {
            if let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = &event
            {
                let count = self.tabs.len();
                if modifiers.ctrl && modifiers.shift && *key == Key::T {
                    self.new_tab();
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == Key::A {
                    self.toggle_launcher();
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == Key::S {
                    self.toggle_settings();
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == Key::M {
                    self.toggle_music();
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == Key::N {
                    self.toggle_notices();
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == Key::P {
                    self.toggle_power();
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && *key == Key::Q {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                    continue;
                }
                if modifiers.ctrl && modifiers.shift && matches!(key, Key::E | Key::O) {
                    self.split_terminal(*key == Key::O);
                    continue;
                }
                // With the terminal split, Ctrl+Shift and an arrow moves between panes.
                let arrow = matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown);
                if modifiers.ctrl && modifiers.shift && arrow && self.side.is_some() {
                    self.side_focused = matches!(key, Key::ArrowRight | Key::ArrowDown);
                    continue;
                }
                if modifiers.ctrl && *key == Key::Tab {
                    // The next tab for the pane in use, passing over the other pane's.
                    let other = if self.side_focused { Some(self.active) } else { self.side };
                    let step = if modifiers.shift { count - 1 } else { 1 };
                    let mut next = self.focus_tab();
                    for _ in 0..count {
                        next = (next + step) % count;
                        if Some(next) != other {
                            break;
                        }
                    }
                    if Some(next) != other {
                        self.show_tab(next);
                    }
                    continue;
                }
            }
            rest.push(event);
        }
        rest
    }
}

/// Where the tabs in view end up once tabs whose shells exited are removed. `alive`
/// says which tabs remain. Returns the new first-pane tab and second-pane tab; the
/// split ends if either of its tabs is gone.
fn remap_panes(alive: &[bool], active: usize, side: Option<usize>) -> (usize, Option<usize>) {
    let moved = |old: usize| alive.get(old).copied().unwrap_or(false).then(|| alive[..old].iter().filter(|a| **a).count());
    let remaining = alive.iter().filter(|a| **a).count();
    match (moved(active), side.and_then(moved)) {
        (Some(active), side) => (active, side),
        // The first pane's tab is gone: the second pane's takes over.
        (None, Some(side)) => (side, None),
        // Neither is left: the tab that was next to the one in view.
        (None, None) => {
            let before = alive[..active.min(alive.len())].iter().filter(|a| **a).count();
            (before.min(remaining.saturating_sub(1)), None)
        }
    }
}

/// Draws one region of the frame: a panel-coloured rectangle with its own layout,
/// clipped to its bounds so that nothing in it can push its neighbours.
fn region<R>(
    ui: &mut Ui,
    rect: Rect,
    margin: egui::Vec2,
    layout: Layout,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.painter().rect_filled(rect, 0.0, ui::PANEL);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(margin)).layout(layout));
    child.set_clip_rect(rect);
    add(&mut child)
}

/// What the top bar asks for in one frame.
#[derive(Default)]
struct BarActions {
    command: Option<Command>,
    open_tab: bool,
    split_terminal: bool,
    toggle_launcher: bool,
}

/// How one tab in the top bar looks.
struct Tab<'a> {
    /// A two-digit position, for terminal tabs.
    number: Option<String>,
    name: &'a str,
    /// The program running in a terminal tab.
    detail: Option<&'a str>,
    /// The display an application is on, when there are several.
    badge: Option<String>,
    selected: bool,
}

/// Draws a tab: a label, underlined in the accent when it is the one in view.
fn tab(ui: &mut Ui, theme: &Theme, tab: Tab) -> Response {
    let ink = if tab.selected { ui::TEXT } else { ui::SECONDARY };
    let quiet = if tab.selected { ui::SECONDARY } else { ui::LABEL };
    let name_font = if tab.selected { ui::display_bold(13.0) } else { ui::display(13.0) };
    let number = tab
        .number
        .map(|n| ui::galley(ui, &n, ui::display(13.0), if tab.selected { theme.main } else { ui::LABEL }, 0.0));
    let name = ui::galley(ui, &tab.name.to_uppercase(), name_font, ink, 1.5);
    let detail = tab.detail.map(|d| ui::galley(ui, d, ui::mono(11.0), quiet, 0.0));
    let badge_ink = if tab.selected { theme.main } else { ui::SECONDARY };
    let badge = tab.badge.map(|b| ui::galley(ui, &b, ui::mono(10.0), badge_ink, 0.0));

    let gap = 8.0;
    let mut width = name.size().x;
    for part in [&number, &detail].into_iter().flatten() {
        width += part.size().x + gap;
    }
    if let Some(badge) = &badge {
        width += badge.size().x + 10.0 + gap;
    }
    let (rect, response) = ui.allocate_exact_size(vec2(width + 28.0, ui.available_height()), Sense::click());
    let painter = ui.painter();
    let mut x = rect.left() + 14.0;
    let mut put = |galley: std::sync::Arc<egui::Galley>, color| {
        let size = galley.size();
        painter.galley(pos2(x, rect.center().y - size.y / 2.0), galley, color);
        x += size.x + gap;
    };
    if let Some(number) = number {
        put(number, ui::LABEL);
    }
    put(name, if response.hovered() { ui::TEXT } else { ink });
    if let Some(detail) = detail {
        put(detail, quiet);
    }
    if let Some(badge) = badge {
        let frame = Rect::from_min_size(
            pos2(x, rect.center().y - badge.size().y / 2.0 - 2.0),
            badge.size() + vec2(10.0, 4.0),
        );
        let edge = if tab.selected { theme.main } else { ui::LINE_STRONG };
        painter.rect_stroke(frame, 0.0, Stroke::new(1.0, edge), StrokeKind::Inside);
        painter.galley(frame.min + vec2(5.0, 2.0), badge, badge_ink);
    }
    if tab.selected {
        let underline = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.max);
        painter.rect_filled(underline, 0.0, theme.main);
    }
    response
}

impl App {
    /// Draws the top bar: terminal tabs, open applications, and the workspace controls.
    fn top_bar(&mut self, ui: &mut Ui, theme: &Theme, state: Option<&DesktopState>) -> BarActions {
        let mut actions = BarActions::default();
        ui.spacing_mut().item_spacing.x = 4.0;
        let covered = state.and_then(|s| s.covering()).is_some();
        let terminal_in_use = !covered && !self.launcher.open && !self.settings.open && !self.music.open && !self.notices.open && !self.power_menu.open;

        for i in 0..self.tabs.len() {
            let process = self.tabs[i]
                .foreground_process()
                .unwrap_or_else(|| "shell".to_string());
            let look = Tab {
                number: Some(format!("{:02}", i + 1)),
                name: if i == 0 { "main" } else { "term" },
                detail: Some(&process),
                badge: None,
                selected: (i == self.active || Some(i) == self.side) && terminal_in_use,
            };
            if tab(ui, theme, look).clicked() {
                self.show_tab(i);
                self.launcher.open = false;
                self.settings.open = false;
                self.music.open = false;
                self.notices.open = false;
                self.power_menu.open = false;
                actions.command = Some(Command::Terminal);
            }
        }
        if self.tabs.len() < MAX_TABS {
            let (rect, response) = ui.allocate_exact_size(vec2(32.0, ui.available_height()), Sense::click());
            let ink = if response.hovered() { ui::TEXT } else { ui::SECONDARY };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "+", ui::mono(16.0), ink);
            if response.on_hover_text("New terminal tab").clicked() {
                actions.open_tab = true;
                self.launcher.open = false;
                actions.command = Some(Command::Terminal);
            }
        }

        let split_hint = if self.side.is_some() {
            "Make the terminal whole again (Ctrl+Shift+E)"
        } else {
            "Split the terminal: Ctrl+Shift+E side by side, Ctrl+Shift+O one above the other"
        };
        if ui::icon_button(ui, theme, Icon::SplitPane, 28.0, self.side.is_some())
            .on_hover_text(split_hint)
            .clicked()
        {
            actions.split_terminal = true;
            self.launcher.open = false;
            self.settings.open = false;
            self.music.open = false;
            self.notices.open = false;
            self.power_menu.open = false;
            actions.command = Some(Command::Terminal);
        }

        if let Some(state) = state.filter(|state| !state.apps.is_empty()) {
            let (rule, _) = ui.allocate_exact_size(vec2(21.0, ui.available_height()), Sense::hover());
            ui.painter()
                .vline(rule.center().x, rule.shrink2(vec2(0.0, 10.0)).y_range(), Stroke::new(1.0, ui::LINE));
            for (i, app) in state.apps.iter().enumerate() {
                let look = Tab {
                    number: None,
                    name: &panels::truncate(&app.title, 20),
                    detail: None,
                    badge: (state.screens > 1).then(|| (app.screen + 1).to_string()),
                    selected: app.place != Place::Hidden
                        && !((self.launcher.open || self.settings.open || self.music.open || self.notices.open || self.power_menu.open) && app.screen == 0),
                };
                if tab(ui, theme, look).clicked() {
                    self.launcher.open = false;
                    self.settings.open = false;
                    self.music.open = false;
                    self.notices.open = false;
                    self.power_menu.open = false;
                    actions.command = Some(Command::Activate(i));
                }
                // The application in use carries its own controls.
                if state.focus == Some(i) {
                    if state.screens > 1
                        && ui::icon_button(ui, theme, Icon::Send, 24.0, false)
                            .on_hover_text("Send to the next display")
                            .clicked()
                    {
                        actions.command = Some(Command::Move(i));
                    }
                    if ui::icon_button(ui, theme, Icon::Close, 24.0, false)
                        .on_hover_text("Close this application")
                        .clicked()
                    {
                        actions.command = Some(Command::Close(i));
                    }
                }
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let hint = if self.launcher.open { "ESC" } else { "SUPER SPACE" };
            if ui::button(ui, theme, "APPS", Some(hint), self.launcher.open).clicked() {
                actions.toggle_launcher = true;
            }
            if let Some(state) = state {
                let split = state.apps.iter().any(|app| app.place == Place::Side);
                if ui::button(ui, theme, "SPLIT", None, split)
                    .on_hover_text("Share the workspace between two windows (Super+S)")
                    .clicked()
                {
                    actions.command = Some(Command::Split);
                }
                if !state.apps.is_empty()
                    && ui::button(ui, theme, "FULL", None, false)
                        .on_hover_text("Let the application cover the whole display (Super+F)")
                        .clicked()
                {
                    actions.command = Some(Command::Fullscreen);
                }
            }
        });
        actions
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
        ui::GROUND.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.mon.tick();
        if let Some(desktop) = &mut self.desktop {
            desktop.pump();
            if std::mem::take(&mut desktop.launcher_requested) && !self.launcher.open {
                self.settings.open = false;
                self.music.open = false;
                self.notices.open = false;
                self.power_menu.open = false;
                self.launcher.open();
            }
            if std::mem::take(&mut desktop.settings_requested) && !self.settings.open {
                self.launcher.open = false;
                self.music.open = false;
                self.notices.open = false;
                self.power_menu.open = false;
                self.settings.open(&self.config);
            }
            if std::mem::take(&mut desktop.music_requested) && !self.music.open {
                self.launcher.open = false;
                self.settings.open = false;
                self.notices.open = false;
                self.power_menu.open = false;
                self.music.open = true;
            }
            if std::mem::take(&mut desktop.power_requested) {
                self.launcher.open = false;
                self.settings.open = false;
                self.music.open = false;
                self.notices.open = false;
                self.power_menu.open = true;
            }
            if std::mem::take(&mut desktop.notifications_requested) && !self.notices.open {
                self.launcher.open = false;
                self.settings.open = false;
                self.music.open = false;
                self.notices.open = true;
            }
            // Bring the frame in line with the saved choice of panels: once at the
            // start, and again whenever that choice is changed in the settings.
            if !self.panels_synced && desktop.connected() {
                let (now, wanted) = (desktop.state.panels, self.config.panels);
                for (panel, differs) in [
                    (khadi_common::Panel::Left, now.left != wanted.left),
                    (khadi_common::Panel::Right, now.right != wanted.right),
                    (khadi_common::Panel::Bottom, now.bottom != wanted.bottom),
                ] {
                    if differs {
                        desktop.send(Command::TogglePanel(panel));
                    }
                }
                self.panels_synced = true;
            }
        }
        for tab in &mut self.tabs {
            tab.pump();
        }
        // Tabs whose shell has exited go; the ones in view keep their places, and a
        // split whose pane lost its tab becomes whole again.
        let alive: Vec<bool> = self.tabs.iter().map(|tab| !tab.exited).collect();
        let (active, side) = remap_panes(&alive, self.active, self.side);
        self.tabs.retain(|tab| !tab.exited);
        if self.tabs.is_empty() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        self.active = active;
        self.side = side;
        self.side_focused &= side.is_some();

        self.handle_screenshot(ctx);

        // The lock screen comes before everything else, and while it is up nothing
        // else is drawn or given any input.
        let (asked, compositor_locked) = match &mut self.desktop {
            Some(desktop) => (std::mem::take(&mut desktop.lock_requested), Some(desktop.locked)),
            None => (false, None),
        };
        if asked && !self.lock.locked {
            self.lock_now();
        }
        match compositor_locked {
            // A shell started while the screen is locked takes the lock over.
            Some(true) if !self.lock.locked => self.lock.engage(),
            Some(true) => self.lock_confirmed = true,
            // Unlocked from outside, with `khadi --unlock`.
            Some(false) if self.lock.locked && self.lock_confirmed => self.lock.release(),
            _ => {}
        }
        self.notices.pump();
        if let Some(desktop) = &mut self.desktop {
            for shot in std::mem::take(&mut desktop.shots) {
                self.capture.take(shot);
            }
            for state in std::mem::take(&mut desktop.recording_news) {
                self.capture.recording_is(state, &mut self.notices);
            }
        }
        self.capture.report(&mut self.notices);
        // Sleep is on its way, or the session was told to lock.
        if self.power.lock_wanted() && !self.lock.locked {
            let for_sleep = self.power.sleep_waiting();
            if !for_sleep || self.config.lock_on_sleep {
                self.lock_now();
            }
        }
        if self.power.sleep_waiting() {
            // The machine may sleep once the lock is what is on screen, or at once if
            // there is to be no lock.
            let up = self.lock.locked && (self.lock_confirmed || self.desktop.is_none());
            if up || !self.lock.locked {
                self.power.let_sleep();
            } else {
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
        let held = self.power.holding();
        if held != self.idle_held {
            self.idle_held = held;
            self.send(Command::HoldIdle(held));
        }
        let screens = self.desktop.as_ref().filter(|desktop| desktop.connected()).map(|desktop| desktop.state.screens);
        if self.watch.check(&mut self.notices, self.mon.power, !self.mon.ip.is_empty(), screens) {
            power::Act::Suspend.run();
        }
        if self.lock.locked {
            self.capture.cancel();
            // Nothing is shown over the lock; what arrives waits in the list.
            if self.toast_area != Some(None) {
                self.toast_area = Some(None);
                self.send(Command::Toast(None));
            }
            let theme = self.theme.clone();
            let music = &mut self.music;
            if self.lock.show(ctx, &theme, |ui, within| music.lock_card(ui, &theme, within)) {
                self.send(Command::Unlock);
            }
            return;
        }
        let events = self.take_shortcuts(ctx);
        let theme = self.theme.clone();
        let screen = ctx.screen_rect();
        let state = self.desktop.as_ref().map(|desktop| desktop.state.clone());
        // The compositor decides which panels are open; alone, the shell does.
        let open = state.as_ref().map_or(self.local_panels, |state| state.panels);
        let places = self.geo.snapshot();
        // Still, the globe faces this machine's side of the world.
        let rotation = if self.config.globe_rotation {
            self.started.elapsed().as_secs_f32() * globe::SPEED
        } else {
            places.home.as_ref().map_or(20.0, |home| -home.lon)
        };
        let overlay = if self.launcher.open {
            Some("LAUNCHER")
        } else if self.settings.open {
            Some("SETTINGS")
        } else if self.music.open {
            Some("MUSIC")
        } else if self.notices.open {
            Some("NOTIFICATIONS")
        } else if self.power_menu.open {
            Some("POWER")
        } else {
            None
        };
        let mut command = None;

        // Every region is placed from the shared geometry, so that the workspace is
        // exactly where the compositor puts application windows.
        let area = khadi_common::workspace(screen.width(), screen.height(), open);
        let workspace = Rect::from_min_size(
            screen.min + vec2(area.x, area.y),
            vec2(area.width, area.height),
        );
        let status_top = screen.bottom() - khadi_common::STATUS_HEIGHT;
        CentralPanel::default()
            .frame(Frame::new().fill(ui::GROUND))
            .show(ctx, |ui| {
                let status = Rect::from_min_max(pos2(screen.left(), status_top), screen.max);
                let toggled = region(ui, status, vec2(24.0, 0.0), Layout::left_to_right(Align::Center), |ui| {
                    panels::status(ui, &theme, &self.mon, state.as_ref(), open, overlay, self.notices.count(), self.capture.recording.map(|since| since.elapsed()))
                });
                let (toggled, settings_clicked, music_clicked, notices_clicked, power_clicked, stop_clicked) = toggled;
                if stop_clicked {
                    self.toggle_recording();
                }
                if power_clicked {
                    self.toggle_power();
                }
                if notices_clicked {
                    self.toggle_notices();
                }
                if settings_clicked {
                    self.toggle_settings();
                }
                if music_clicked {
                    self.toggle_music();
                }
                if let Some(panel) = toggled {
                    match &self.desktop {
                        Some(_) => command = Some(Command::TogglePanel(panel)),
                        None => self.local_panels.toggle(panel),
                    }
                }

                let column = vec2(24.0, 24.0);
                if open.left {
                    let rect = Rect::from_min_max(screen.min, pos2(workspace.left(), status_top));
                    region(ui, rect, column, Layout::top_down(Align::Min), |ui| {
                        panels::left(ui, &theme, &self.mon)
                    });
                }
                if open.right {
                    let rect = Rect::from_min_max(pos2(workspace.right(), screen.top()), pos2(screen.right(), status_top));
                    region(ui, rect, column, Layout::top_down(Align::Min), |ui| {
                        let music = &mut self.music;
                        let mut wanted = false;
                        panels::right(ui, &theme, &self.mon, state.as_ref(), rotation, &places, |ui| {
                            wanted = music.panel(ui, &theme);
                        });
                        wanted
                    })
                    .then(|| self.toggle_music());
                }
                let mut typed = None;
                if open.bottom {
                    let rect = Rect::from_min_max(workspace.left_bottom(), pos2(workspace.right(), status_top));
                    typed = region(ui, rect, vec2(24.0, 16.0), Layout::top_down(Align::Min), |ui| {
                        let cwd = self.tabs[self.focus_tab()].cwd();
                        self.fs.show(ui, &theme, cwd.as_deref())
                    });
                }

                // The top bar stays in view while an application covers what is below
                // it, so it is also the way back to the terminal.
                let bar = Rect::from_min_max(pos2(workspace.left(), screen.top()), workspace.right_top());
                let actions = region(ui, bar, vec2(16.0, 0.0), Layout::left_to_right(Align::Center), |ui| {
                    self.top_bar(ui, &theme, state.as_ref())
                });
                if actions.open_tab {
                    self.new_tab();
                }
                if actions.split_terminal {
                    self.split_terminal(false);
                }
                if actions.toggle_launcher {
                    self.toggle_launcher();
                }
                command = actions.command.or(command.take());

                let mut mine = workspace;
                if state.as_ref().is_some_and(|state| state.terminal_split) {
                    // An application has the right half; keep the terminal out from under it.
                    let (left, _) = area.halves();
                    mine.set_width(left.width);
                }
                let inner = mine.shrink2(vec2(24.0, 18.0));
                let inner = Rect::from_min_max(inner.min, inner.max.max(inner.min));

                if self.settings.open {
                    let changes = region(ui, mine, vec2(0.0, 0.0), Layout::top_down(Align::Min), |ui| {
                        let displays = self.desktop.as_ref().map(|desktop| &desktop.displays);
                        self.settings.show(ui, mine, &theme, &self.config, open, &self.mon, &places, displays, &self.input)
                    });
                    for change in changes {
                        self.apply(ctx, change);
                    }
                    return;
                }
                if self.music.open {
                    region(ui, mine, vec2(0.0, 0.0), Layout::top_down(Align::Min), |ui| {
                        self.music.show(ui, mine, &theme)
                    });
                    return;
                }
                if self.power_menu.open {
                    let act = region(ui, mine, vec2(0.0, 0.0), Layout::top_down(Align::Min), |ui| {
                        self.power_menu.show(ui, mine, &theme, self.mon.power)
                    });
                    match act {
                        Some(power::Act::Lock) => self.lock_now(),
                        Some(power::Act::LogOut) => ctx.send_viewport_cmd(ViewportCommand::Close),
                        Some(act) => act.run(),
                        None => {}
                    }
                    return;
                }
                if self.notices.open {
                    let (chosen, quiet) = region(ui, mine, vec2(0.0, 0.0), Layout::top_down(Align::Min), |ui| {
                        self.notices.show(ui, mine, &theme)
                    });
                    if let Some(quiet) = quiet {
                        if let Err(e) = config::set("do_not_disturb", quiet.then_some("on")) {
                            eprintln!("khadi: cannot save the setting: {e}");
                        }
                    }
                    if let Some(chosen) = chosen {
                        self.follow(chosen);
                    }
                    return;
                }
                if self.launcher.open {
                    let picked = region(ui, mine, vec2(0.0, 0.0), Layout::top_down(Align::Min), |ui| {
                        self.launcher.show(ui, mine, &theme)
                    });
                    if let Some(entry) = picked {
                        self.run_in_terminal(&entry);
                    }
                    return;
                }

                // One pane, or two with a gap and a rule between them.
                let panes: Vec<(usize, Rect)> = match self.side {
                    None => vec![(self.active, inner)],
                    Some(side) => {
                        let gap = 25.0;
                        let (first, second) = if self.stacked {
                            let height = ((inner.height() - gap) / 2.0).floor().max(1.0);
                            let rule = inner.top() + height + gap / 2.0;
                            ui.painter().hline(inner.x_range(), rule, Stroke::new(1.0, ui::LINE));
                            (
                                Rect::from_min_size(inner.min, vec2(inner.width(), height)),
                                Rect::from_min_max(pos2(inner.left(), inner.top() + height + gap), inner.max),
                            )
                        } else {
                            let width = ((inner.width() - gap) / 2.0).floor().max(1.0);
                            let rule = inner.left() + width + gap / 2.0;
                            ui.painter().vline(rule, inner.y_range(), Stroke::new(1.0, ui::LINE));
                            (
                                Rect::from_min_size(inner.min, vec2(width, inner.height())),
                                Rect::from_min_max(pos2(inner.left() + width + gap, inner.top()), inner.max),
                            )
                        };
                        vec![(self.active, first), (side, second)]
                    }
                };
                let responses: Vec<Response> = panes
                    .iter()
                    .enumerate()
                    .map(|(n, (_, rect))| ui.interact(*rect, ui.id().with(("terminal-pane", n)), Sense::click_and_drag()))
                    .collect();
                // A click in a pane gives it the keyboard.
                for (n, response) in responses.iter().enumerate() {
                    if panes.len() > 1 && (response.clicked() || response.drag_started()) {
                        self.side_focused = n == 1;
                    }
                }
                let focused = if self.side_focused && panes.len() > 1 { 1 } else { 0 };
                if panes.len() > 1 {
                    // A mark over the pane in use.
                    let rect = panes[focused].1;
                    let mark = Rect::from_min_size(pos2(rect.left(), rect.top() - 10.0), vec2(36.0, 2.0));
                    ui.painter().rect_filled(mark, 0.0, theme.main);
                }
                let response = &responses[focused];

                // The terminal owns the keyboard: keep focus on it and stop egui from
                // using Tab, arrows and Escape for its own widget navigation.
                ui.memory_mut(|m| {
                    m.request_focus(response.id);
                    m.set_focus_lock_filter(
                        response.id,
                        EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: true,
                        },
                    );
                });

                let (alt_held, shift_held) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
                let tab = &mut self.tabs[panes[focused].0];
                tab.handle_events(&events, alt_held, shift_held, ctx);
                if let Some(text) = typed.take() {
                    tab.write(text.as_bytes());
                }
                let look = self.config.term_look();
                for (n, (index, rect)) in panes.iter().enumerate() {
                    self.tabs[*index].show(ui, *rect, &theme, &responses[n], &look, n == focused);
                }
            });
        if let (Some(command), Some(desktop)) = (command, self.desktop.as_mut()) {
            desktop.send(command);
        }

        // The one-pixel lines between the frame's regions, drawn over all of them.
        let lines = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("frame-lines")));
        let line = Stroke::new(1.0, ui::LINE);
        lines.hline(screen.x_range(), status_top, line);
        if open.left {
            lines.vline(workspace.left(), screen.top()..=status_top, line);
        }
        if open.right {
            lines.vline(workspace.right(), screen.top()..=status_top, line);
        }
        if workspace.is_positive() {
            lines.hline(workspace.x_range(), workspace.top(), line);
            if open.bottom {
                lines.hline(workspace.x_range(), workspace.bottom(), line);
            }
        }

        // Toasts hang from the top right corner of the workspace. The compositor is
        // told where, so that it shows them above whatever application is open.
        let (mut covered, chosen) = self.notices.toasts(ctx, &theme, workspace.right_top() + vec2(-16.0, 12.0));
        // Marking out a piece for a screenshot takes the whole display, above everything.
        if self.capture.marking() {
            if self.capture.mark(ctx, theme.main) {
                self.toggle_recording();
            }
            covered = Some(screen);
        }
        let area = covered.map(|rect| {
            (rect.left().floor() as i32, rect.top().floor() as i32, rect.width().ceil() as i32 + 1, rect.height().ceil() as i32 + 1)
        });
        if self.toast_area != Some(area) {
            self.toast_area = Some(area);
            self.send(Command::Toast(area));
        }
        if let Some(chosen) = chosen {
            self.follow(chosen);
        }

        // Every further display gets a window of its own, which the compositor puts
        // behind the applications there.
        let screens = state.as_ref().map_or(1, |state| state.screens);
        for number in 2..=screens {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(("khadi-display", number)),
                ViewportBuilder::default()
                    .with_title(format!("khadi display {number}"))
                    .with_app_id("khadi"),
                |ctx, class| {
                    // Without support for extra windows there is nowhere to draw this.
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }
                    CentralPanel::default()
                        .frame(Frame::new().fill(ui::GROUND))
                        .show(ctx, |ui| panels::backdrop(ui, &theme, &self.mon, number));
                },
            );
        }

        // The globe turns continuously; without it, the clock sets the pace.
        let pace = if open.right && self.config.globe_rotation { 66 } else { 500 };
        ctx.request_repaint_after(Duration::from_millis(pace));
    }
}

/// Finds an installed monospace font that has prompt glyphs (a Nerd Font).
fn find_nerd_font() -> Option<PathBuf> {
    let output = std::process::Command::new("fc-list")
        .arg("--format=%{file}\t%{family}\t%{style}\n")
        .output()
        .ok()?;
    let listing = String::from_utf8_lossy(&output.stdout);
    let mut candidates: Vec<&str> = listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let (file, family, style) = (fields.next()?, fields.next()?, fields.next()?);
            let nerd = family.contains("Nerd Font") || family.split(',').any(|f| f.ends_with(" NF"));
            (nerd && style.split(',').any(|s| s == "Regular")).then_some(file)
        })
        .collect();
    candidates.sort();
    candidates.first().map(PathBuf::from)
}

/// Runs a helper program without waiting for it or showing what it prints.
fn spawn_quietly(program: &str, args: &[&str]) {
    let spawned = std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("khadi: cannot run {program}: {e}"),
    }
}

/// Sets the backlight to a percentage of its range, through the login service, which
/// lets the active session do that without special permissions.
fn set_brightness(percent: u8) {
    let Some(device) = std::fs::read_dir("/sys/class/backlight")
        .ok()
        .and_then(|mut dir| dir.next()?.ok())
    else {
        return;
    };
    let max: Option<u64> = std::fs::read_to_string(device.path().join("max_brightness"))
        .ok()
        .and_then(|text| text.trim().parse().ok());
    let Some(max) = max else {
        return;
    };
    let target = (max * percent.min(100) as u64 / 100).max(1);
    spawn_quietly(
        "busctl",
        &[
            "call",
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
            "SetBrightness",
            "ssu",
            "backlight",
            &device.file_name().to_string_lossy(),
            &target.to_string(),
        ],
    );
}

fn apply_style(ctx: &egui::Context, theme: &Theme) {
    let mut style = (*ctx.style()).clone();
    style.override_font_id = Some(ui::mono(12.0));
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(ui::TEXT);
    visuals.panel_fill = ui::GROUND;
    visuals.window_fill = ui::PANEL;
    visuals.extreme_bg_color = ui::GROUND;
    visuals.selection.bg_fill = theme.selection();
    visuals.selection.stroke = Stroke::new(1.0, theme.main);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, ui::LINE);
    visuals.widgets.hovered.weak_bg_fill = ui::TRACK;
    visuals.widgets.active.weak_bg_fill = ui::LINE;
    visuals.window_stroke = Stroke::new(1.0, ui::LINE_STRONG);
    style.visuals = visuals;
    ctx.set_style(style);
}

fn main() -> eframe::Result {
    let mut windowed = false;
    let mut screenshot = None;
    let mut font = None;
    let mut launcher = false;
    let mut lyrics = false;
    let mut music = false;
    let mut power_menu = false;
    let mut greeter = false;
    let mut locked = false;
    let mut split = None;
    let mut settings: Option<String> = None;
    let mut config = config::Config::load();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--windowed" => windowed = true,
            "--launcher" => launcher = true,
            "--music" => music = true,
            "--music=lyrics" => {
                music = true;
                lyrics = true;
            }
            "--power" => power_menu = true,
            "--greeter" => greeter = true,
            "--locked" => locked = true,
            "--split-terminal" => split = Some(false),
            "--split-terminal=below" => split = Some(true),
            "--unlock" => {
                // Every compositor of this user's: there is normally one.
                let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_default();
                let mut reached = 0;
                for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with("khadi-comp-") && name.ends_with(".sock") {
                        if let Ok(mut stream) = std::os::unix::net::UnixStream::connect(entry.path()) {
                            use std::io::Write;
                            if stream.write_all(Command::Unlock.encode().as_bytes()).is_ok() {
                                reached += 1;
                            }
                        }
                    }
                }
                if reached == 0 {
                    eprintln!("khadi: found no running desktop to unlock in {}", dir.display());
                    std::process::exit(1);
                }
                println!("Unlocked.");
                return Ok(());
            }
            "--write-boot-splash" => {
                let outcome = match args.next() {
                    Some(dir) => {
                        let config = config::Config::load();
                        bootsplash::write(std::path::Path::new(&dir), &Theme::load(&config.theme))
                    }
                    None => Err("--write-boot-splash needs a folder".to_string()),
                };
                match outcome {
                    Ok(count) => println!("Wrote {count} pictures for the boot splash."),
                    Err(e) => {
                        eprintln!("khadi: {e}");
                        std::process::exit(1);
                    }
                }
                return Ok(());
            }
            "--set-avatar" => {
                let outcome = match args.next() {
                    Some(file) => greeter::set_avatar(std::path::Path::new(&file)),
                    None => Err("--set-avatar needs a picture file".to_string()),
                };
                match outcome {
                    Ok(path) => println!("Saved to {}. The login screen shows it from now on.", path.display()),
                    Err(e) => {
                        eprintln!("khadi: {e}");
                        std::process::exit(1);
                    }
                }
                return Ok(());
            }
            "--settings" => settings = Some(String::new()),
            section if section.starts_with("--settings=") => {
                settings = Some(section["--settings=".len()..].to_string());
            }
            "--set-location" | "--geo" => {
                let (key, value) = match (arg.as_str(), args.next()) {
                    ("--set-location", Some(value)) if value == "auto" => ("location", None),
                    ("--set-location", Some(value)) if geo::parse_location(&value).is_some() => {
                        ("location", Some(value))
                    }
                    ("--geo", Some(value)) if value == "on" => ("geo", None),
                    ("--geo", Some(value)) if value == "off" => ("geo", Some(value)),
                    _ => {
                        eprintln!("khadi: {arg} needs a valid value\n\n{USAGE}");
                        std::process::exit(2);
                    }
                };
                match config::set(key, value.as_deref()) {
                    Ok(path) => println!("Saved to {}. It takes effect when the shell next starts.", path.display()),
                    Err(e) => {
                        eprintln!("khadi: cannot save the setting: {e}");
                        std::process::exit(1);
                    }
                }
                return Ok(());
            }
            "--list-apps" => {
                for entry in apps::scan() {
                    let terminal = if entry.terminal { " (terminal)" } else { "" };
                    println!("{}{terminal}\t{}", entry.name, entry.exec.join(" "));
                }
                return Ok(());
            }
            "--theme" => match args.next() {
                Some(name) => config.theme = name,
                None => {
                    eprintln!("khadi: --theme needs a value\n\n{USAGE}");
                    std::process::exit(2);
                }
            },
            "--font" => match args.next() {
                Some(path) => font = Some(PathBuf::from(path)),
                None => {
                    eprintln!("khadi: --font needs a value\n\n{USAGE}");
                    std::process::exit(2);
                }
            },
            "--screenshot" => match args.next() {
                Some(path) => screenshot = Some(PathBuf::from(path)),
                None => {
                    eprintln!("khadi: --screenshot needs a value\n\n{USAGE}");
                    std::process::exit(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => {
                eprintln!("khadi: unknown option '{other}'\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let theme = Theme::load(&config.theme);
    if greeter {
        let options = eframe::NativeOptions {
            viewport: ViewportBuilder::default()
                .with_title("Khadi login")
                .with_app_id("khadi-greeter")
                .with_inner_size([1280.0, 760.0])
                .with_fullscreen(!windowed),
            ..Default::default()
        };
        return eframe::run_native(
            "khadi-greeter",
            options,
            Box::new(move |cc| {
                apply_style(&cc.egui_ctx, &theme);
                ui::install_fonts(&cc.egui_ctx, None, None);
                Ok(Box::new(greeter::Greeter::new(theme, screenshot)))
            }),
        );
    }
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("khadi")
            .with_app_id("khadi")
            .with_inner_size([1280.0, 760.0])
            .with_fullscreen(!windowed),
        ..Default::default()
    };
    eframe::run_native(
        "khadi",
        options,
        Box::new(move |cc| {
            apply_style(&cc.egui_ctx, &theme);
            let symbols = font.or_else(find_nerd_font);
            ui::install_fonts(&cc.egui_ctx, symbols.as_deref(), config.terminal_font.as_deref());
            let mut app = App::new(cc.egui_ctx.clone(), theme, config, symbols, screenshot)?;
            if locked {
                app.lock.engage();
            }
            if let Some(stacked) = split {
                app.split_terminal(stacked);
            }
            if let Some(section) = &settings {
                app.settings.open_at(&app.config.clone(), section);
            }
            if launcher {
                app.launcher.open();
            }
            app.music.open = music;
            if lyrics {
                app.music.show_lyrics();
            }
            app.power_menu.open = power_menu;
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::remap_panes;

    #[test]
    fn tabs_in_view_keep_their_places_when_others_close() {
        // Nothing closed.
        assert_eq!(remap_panes(&[true, true, true], 1, Some(2)), (1, Some(2)));
        // A tab to the left closes: both shift down.
        assert_eq!(remap_panes(&[false, true, true], 1, Some(2)), (0, Some(1)));
        // A tab between them closes.
        assert_eq!(remap_panes(&[true, false, true], 0, Some(2)), (0, Some(1)));
    }

    #[test]
    fn a_split_ends_when_one_of_its_tabs_closes() {
        // The second pane's tab closes: whole again, on the first.
        assert_eq!(remap_panes(&[true, true, false], 1, Some(2)), (1, None));
        // The first pane's tab closes: the second pane's tab takes over.
        assert_eq!(remap_panes(&[true, false, true], 1, Some(2)), (1, None));
        // Both close: the nearest tab left.
        assert_eq!(remap_panes(&[true, false, false], 1, Some(2)), (0, None));
        assert_eq!(remap_panes(&[false, false, true], 0, Some(1)), (0, None));
    }

    #[test]
    fn without_a_split_the_tab_in_view_follows_its_neighbours() {
        assert_eq!(remap_panes(&[true, false, true], 1, None), (1, None));
        assert_eq!(remap_panes(&[true, true, false], 2, None), (1, None));
        assert_eq!(remap_panes(&[false, true], 1, None), (0, None));
    }
}
