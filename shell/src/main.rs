//! edex-rs: a Rust rewrite of the eDEX-UI sci-fi terminal desktop.

mod apps;
mod config;
mod desktop;
mod geo;
mod globe;
mod launcher;
mod panels;
mod radio;
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
use edex_common::{
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
Usage: edex-rs [OPTIONS]

Options:
  --theme <NAME|FILE>  Theme to use: its accent and terminal colours (default: signal)
  --font <FILE>        Font for prompt symbols (default: an installed Nerd Font, if any)
  --windowed           Run in a window instead of fullscreen
  --launcher           Start with the application launcher open
  --settings[=SECTION] Start with the settings open, optionally on a section
                       (appearance, terminal, frame, displays, network, bluetooth,
                       keyboard, mouse, globe, sound, session)
  --list-apps          Print the applications the launcher would offer, and exit
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
    active: usize,
    mon: SysMon,
    fs: FsView,
    /// The compositor connection; `None` when running as an ordinary window.
    desktop: Option<Desktop>,
    launcher: Launcher,
    settings: settings::Settings,
    config: config::Config,
    /// The keyboard, mouse and touchpad settings, as last saved.
    input: edex_common::input::InputSettings,
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
        Ok(App {
            ctx,
            theme,
            shell,
            tabs: vec![first],
            active: 0,
            mon: SysMon::new(),
            fs: FsView::new(),
            desktop,
            launcher: Launcher::default(),
            settings: settings::Settings::default(),
            geo: geo::Geo::start(config.lookups, config.location.clone()),
            local_panels: config.panels,
            config,
            input: edex_common::input::InputSettings::load(),
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
            .get(self.active)
            .and_then(|t| t.cwd())
            .unwrap_or_else(|| PathBuf::from("/"));
        match TermTab::spawn(&self.shell, &cwd, self.ctx.clone(), self.config.scrollback) {
            Ok(tab) => {
                self.tabs.push(tab);
                self.active = self.tabs.len() - 1;
            }
            Err(e) => eprintln!("edex-rs: cannot open tab: {e}"),
        }
    }

    /// Opens or closes the settings, which like the launcher take the terminal's place.
    fn toggle_settings(&mut self) {
        if self.settings.open {
            self.settings.open = false;
            return;
        }
        self.launcher.open = false;
        self.settings.open(&self.config);
        if let Some(desktop) = &mut self.desktop {
            desktop.send(Command::Terminal);
        }
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
                eprintln!("edex-rs: cannot save the setting: {e}");
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
                    edex_common::Panel::Left => panels.left = open,
                    edex_common::Panel::Right => panels.right = open,
                    edex_common::Panel::Bottom => panels.bottom = open,
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
                self.input = edex_common::input::InputSettings::load();
                if self.config.terminal_font != font {
                    ui::install_fonts(ctx, self.symbols.as_deref(), self.config.terminal_font.as_deref());
                }
                // The compositor owns the devices these are about.
                if ["keyboard_", "mouse_", "touchpad_"].iter().any(|prefix| key.starts_with(prefix)) {
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
            Change::LogOut => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    /// Opens or closes the launcher. It is drawn where the terminal is, so anything
    /// covering that must make way.
    fn toggle_launcher(&mut self) {
        self.settings.open = false;
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
        self.tabs[self.active].write(line.as_bytes());
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
                eprintln!("edex-rs: cannot write {}: {e}", path.display());
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
                if modifiers.ctrl && modifiers.shift && *key == Key::Q {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                    continue;
                }
                if modifiers.ctrl && *key == Key::Tab {
                    self.active = if modifiers.shift {
                        (self.active + count - 1) % count
                    } else {
                        (self.active + 1) % count
                    };
                    continue;
                }
            }
            rest.push(event);
        }
        rest
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
        let terminal_in_use = !covered && !self.launcher.open && !self.settings.open;

        for i in 0..self.tabs.len() {
            let process = self.tabs[i]
                .foreground_process()
                .unwrap_or_else(|| "shell".to_string());
            let look = Tab {
                number: Some(format!("{:02}", i + 1)),
                name: if i == 0 { "main" } else { "term" },
                detail: Some(&process),
                badge: None,
                selected: i == self.active && terminal_in_use,
            };
            if tab(ui, theme, look).clicked() {
                self.active = i;
                self.launcher.open = false;
                self.settings.open = false;
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
                        && !((self.launcher.open || self.settings.open) && app.screen == 0),
                };
                if tab(ui, theme, look).clicked() {
                    self.launcher.open = false;
                    self.settings.open = false;
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
                self.launcher.open();
            }
            if std::mem::take(&mut desktop.settings_requested) && !self.settings.open {
                self.launcher.open = false;
                self.settings.open(&self.config);
            }
            // Bring the frame in line with the saved choice of panels: once at the
            // start, and again whenever that choice is changed in the settings.
            if !self.panels_synced && desktop.connected() {
                let (now, wanted) = (desktop.state.panels, self.config.panels);
                for (panel, differs) in [
                    (edex_common::Panel::Left, now.left != wanted.left),
                    (edex_common::Panel::Right, now.right != wanted.right),
                    (edex_common::Panel::Bottom, now.bottom != wanted.bottom),
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
        // Closing a tab to the left of the active one must not change which tab is shown.
        let mut index = 0;
        let mut active = self.active;
        self.tabs.retain(|tab| {
            if tab.exited && index < self.active {
                active -= 1;
            }
            index += 1;
            !tab.exited
        });
        if self.tabs.is_empty() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        self.active = active.min(self.tabs.len() - 1);

        self.handle_screenshot(ctx);
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
        } else {
            None
        };
        let mut command = None;

        // Every region is placed from the shared geometry, so that the workspace is
        // exactly where the compositor puts application windows.
        let area = edex_common::workspace(screen.width(), screen.height(), open);
        let workspace = Rect::from_min_size(
            screen.min + vec2(area.x, area.y),
            vec2(area.width, area.height),
        );
        let status_top = screen.bottom() - edex_common::STATUS_HEIGHT;
        CentralPanel::default()
            .frame(Frame::new().fill(ui::GROUND))
            .show(ctx, |ui| {
                let status = Rect::from_min_max(pos2(screen.left(), status_top), screen.max);
                let toggled = region(ui, status, vec2(24.0, 0.0), Layout::left_to_right(Align::Center), |ui| {
                    panels::status(ui, &theme, &self.mon, state.as_ref(), open, overlay)
                });
                let (toggled, settings_clicked) = toggled;
                if settings_clicked {
                    self.toggle_settings();
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
                        panels::right(ui, &theme, &self.mon, state.as_ref(), rotation, &places)
                    });
                }
                let mut typed = None;
                if open.bottom {
                    let rect = Rect::from_min_max(workspace.left_bottom(), pos2(workspace.right(), status_top));
                    typed = region(ui, rect, vec2(24.0, 16.0), Layout::top_down(Align::Min), |ui| {
                        let cwd = self.tabs[self.active].cwd();
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
                let response = ui.allocate_rect(inner, Sense::click_and_drag());

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
                if self.launcher.open {
                    let picked = region(ui, mine, vec2(0.0, 0.0), Layout::top_down(Align::Min), |ui| {
                        self.launcher.show(ui, mine, &theme)
                    });
                    if let Some(entry) = picked {
                        self.run_in_terminal(&entry);
                    }
                    return;
                }

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
                let tab = &mut self.tabs[self.active];
                tab.handle_events(&events, alt_held, shift_held, ctx);
                if let Some(text) = typed.take() {
                    tab.write(text.as_bytes());
                }
                tab.show(ui, inner, &theme, &response, &self.config.term_look());
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

        // Every further display gets a window of its own, which the compositor puts
        // behind the applications there.
        let screens = state.as_ref().map_or(1, |state| state.screens);
        for number in 2..=screens {
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(("edex-display", number)),
                ViewportBuilder::default()
                    .with_title(format!("edex-rs display {number}"))
                    .with_app_id("edex-rs"),
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
        Err(e) => eprintln!("edex-rs: cannot run {program}: {e}"),
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
    let mut settings: Option<String> = None;
    let mut config = config::Config::load();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--windowed" => windowed = true,
            "--launcher" => launcher = true,
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
                        eprintln!("edex-rs: {arg} needs a valid value\n\n{USAGE}");
                        std::process::exit(2);
                    }
                };
                match config::set(key, value.as_deref()) {
                    Ok(path) => println!("Saved to {}. It takes effect when the shell next starts.", path.display()),
                    Err(e) => {
                        eprintln!("edex-rs: cannot save the setting: {e}");
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
                    eprintln!("edex-rs: --theme needs a value\n\n{USAGE}");
                    std::process::exit(2);
                }
            },
            "--font" => match args.next() {
                Some(path) => font = Some(PathBuf::from(path)),
                None => {
                    eprintln!("edex-rs: --font needs a value\n\n{USAGE}");
                    std::process::exit(2);
                }
            },
            "--screenshot" => match args.next() {
                Some(path) => screenshot = Some(PathBuf::from(path)),
                None => {
                    eprintln!("edex-rs: --screenshot needs a value\n\n{USAGE}");
                    std::process::exit(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => {
                eprintln!("edex-rs: unknown option '{other}'\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    let theme = Theme::load(&config.theme);
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("edex-rs")
            .with_app_id("edex-rs")
            .with_inner_size([1280.0, 760.0])
            .with_fullscreen(!windowed),
        ..Default::default()
    };
    eframe::run_native(
        "edex-rs",
        options,
        Box::new(move |cc| {
            apply_style(&cc.egui_ctx, &theme);
            let symbols = font.or_else(find_nerd_font);
            ui::install_fonts(&cc.egui_ctx, symbols.as_deref(), config.terminal_font.as_deref());
            let mut app = App::new(cc.egui_ctx.clone(), theme, config, symbols, screenshot)?;
            if let Some(section) = &settings {
                app.settings.open_at(&app.config.clone(), section);
            }
            if launcher {
                app.launcher.open();
            }
            Ok(Box::new(app))
        }),
    )
}
