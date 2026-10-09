//! The settings screen, shown in place of the terminal. It only draws and reports what
//! the user asked for; applying and saving a change is the caller's job.

use crate::config::{Config, LINE_HEIGHT_RANGE, MAX_FONT_SIZE, MIN_FONT_SIZE, SCROLLBACK_CHOICES};
use crate::radio::Radio;
use crate::term::CursorShape;
use khadi_common::input::{InputSettings, REPEAT_DELAY_RANGE, REPEAT_RATE_RANGE};
use crate::geo::{Place, Snapshot, parse_location};
use crate::sysmon::SysMon;
use crate::theme::{self, Theme};
use crate::ui;
use khadi_common::{
    Panel, Panels,
    ipc::{DisplayChoice, DisplayInfo, Displays, Mode},
};
use eframe::egui::{
    Align, Align2, Color32, Key, Layout, Modifiers, Rect, ScrollArea, Sense, Stroke, StrokeKind,
    TextEdit, Ui, UiBuilder, vec2,
};
use std::time::{Duration, Instant};

/// Something the user changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Theme(String),
    FontSize(f32),
    GlobeRotation(bool),
    Lookups(bool),
    /// `None` goes back to guessing from the machine's address.
    Location(Option<Place>),
    /// Open or collapse a panel, now and at every start.
    Panel(Panel, bool),
    /// Percent.
    Volume(u8),
    Brightness(u8),
    /// Save a plain setting in the configuration file; `None` goes back to its default.
    Set(&'static str, Option<String>),
    /// Try this arrangement of the monitors, listed left to right.
    ApplyDisplays(Vec<DisplayChoice>),
    KeepDisplays,
    RevertDisplays,
    LogOut,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Appearance,
    Terminal,
    Frame,
    Displays,
    Network,
    Bluetooth,
    Keyboard,
    Pointer,
    Globe,
    Sound,
    Session,
}

const SECTIONS: [(Section, &str); 11] = [
    (Section::Appearance, "APPEARANCE"),
    (Section::Terminal, "TERMINAL"),
    (Section::Frame, "FRAME"),
    (Section::Displays, "DISPLAYS"),
    (Section::Network, "NETWORK"),
    (Section::Bluetooth, "BLUETOOTH"),
    (Section::Keyboard, "KEYBOARD"),
    (Section::Pointer, "MOUSE & TOUCHPAD"),
    (Section::Globe, "GLOBE"),
    (Section::Sound, "SOUND & SCREEN"),
    (Section::Session, "SESSION"),
];

pub struct Settings {
    pub open: bool,
    section: Section,
    /// Installed themes and their accents, read when the screen opens.
    themes: Vec<(String, Color32)>,
    location_text: String,
    location_error: bool,
    /// A level the user just set, shown until the system reports it back.
    volume_held: Option<(u8, Instant)>,
    brightness_held: Option<(u8, Instant)>,
    /// Logging out takes two clicks; this is when the first one happened.
    log_out_armed: Option<Instant>,
    /// The arrangement being edited, left to right, and the report it started from.
    draft: Vec<DisplayChoice>,
    draft_basis: Vec<DisplayInfo>,
    /// Which monitor of the draft the controls act on.
    selected_display: usize,
    /// Monospace fonts found on this machine, read when the screen opens.
    fonts: Vec<(String, std::path::PathBuf)>,
    layout_text: String,
    radio: Option<Radio>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            open: false,
            section: Section::Appearance,
            themes: Vec::new(),
            location_text: String::new(),
            location_error: false,
            volume_held: None,
            brightness_held: None,
            log_out_armed: None,
            draft: Vec::new(),
            draft_basis: Vec::new(),
            selected_display: 0,
            fonts: Vec::new(),
            layout_text: String::new(),
            radio: None,
        }
    }
}

/// The arrangement the compositor reports, as choices.
fn current_choices(monitors: &[DisplayInfo]) -> Vec<DisplayChoice> {
    monitors
        .iter()
        .map(|monitor| DisplayChoice {
            name: monitor.name.clone(),
            mode: monitor.current,
            main: monitor.main,
        })
        .collect()
}

/// The sizes a monitor offers, largest first, without repeats.
fn sizes(monitor: &DisplayInfo) -> Vec<(u32, u32)> {
    let mut sizes: Vec<(u32, u32)> = Vec::new();
    for mode in &monitor.modes {
        if !sizes.contains(&(mode.width, mode.height)) {
            sizes.push((mode.width, mode.height));
        }
    }
    sizes
}

/// The best mode a monitor offers at a size: the fastest.
fn best_mode(monitor: &DisplayInfo, size: (u32, u32)) -> Option<Mode> {
    monitor
        .modes
        .iter()
        .filter(|mode| (mode.width, mode.height) == size)
        .max_by_key(|mode| mode.refresh)
        .copied()
}

/// A refresh rate the way people say it: 60 Hz, 59.94 Hz.
fn hertz(refresh: u32) -> String {
    let hz = refresh as f32 / 1000.0;
    if (hz - hz.round()).abs() < 0.006 {
        format!("{hz:.0} Hz")
    } else {
        format!("{hz:.2} Hz")
    }
}

/// A level that follows the system's reading, except just after the user moved it.
fn held(slot: &mut Option<(u8, Instant)>, reading: Option<u8>) -> Option<u8> {
    match slot {
        Some((value, at)) if at.elapsed() < Duration::from_secs(4) => Some(*value),
        _ => {
            *slot = None;
            reading
        }
    }
}

impl Settings {
    pub fn open(&mut self, config: &Config) {
        self.themes = theme::installed();
        self.fonts = ui::monospace_fonts();
        self.location_text = match &config.location {
            Some(place) => format!("{}, {}, {}", place.lat, place.lon, place.label),
            None => String::new(),
        };
        self.location_error = false;
        self.log_out_armed = None;
        self.open = true;
    }

    /// Opens on the section with this name, as the `--settings=<name>` option asks.
    pub fn open_at(&mut self, config: &Config, name: &str) {
        self.open(config);
        let wanted = name.to_uppercase();
        if let Some((section, _)) = SECTIONS.iter().find(|(_, title)| title.starts_with(&wanted)) {
            self.section = *section;
        }
    }

    /// Draws the screen into `rect` and returns what was changed this frame.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        theme: &Theme,
        config: &Config,
        panels: Panels,
        mon: &SysMon,
        geo: &Snapshot,
        displays: Option<&Displays>,
        input: &InputSettings,
    ) -> Vec<Change> {
        let mut changes = Vec::new();
        // Escape closes, unless it is being used to leave the text field.
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.open = false;
            return changes;
        }

        let margin = (rect.width() * 0.06).clamp(16.0, 72.0);
        let area = Rect::from_min_max(rect.min + vec2(margin, 32.0), rect.max - vec2(margin, 16.0));
        ui.allocate_new_ui(UiBuilder::new().max_rect(area), |ui| {
            ui.horizontal(|ui| {
                ui::tracked(ui, "SETTINGS", ui::display_bold(22.0), ui::TEXT, 3.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::button(ui, theme, "DONE", Some("ESC"), false).clicked() {
                        self.open = false;
                    }
                });
            });
            ui.add_space(18.0);

            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 32.0;
                // The sections, as a list down the left.
                ui.allocate_ui_with_layout(vec2(200.0, ui.available_height()), Layout::top_down(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (section, name) in SECTIONS {
                        let (row, response) = ui.allocate_exact_size(vec2(200.0, 36.0), Sense::click());
                        let chosen = self.section == section;
                        let painter = ui.painter();
                        if chosen {
                            painter.rect_filled(row, 0.0, theme.main);
                        } else if response.hovered() {
                            painter.rect_filled(row, 0.0, ui::PANEL);
                        }
                        let (font, ink) = if chosen {
                            (ui::display_bold(13.0), ui::GROUND)
                        } else {
                            (ui::display(13.0), ui::SECONDARY)
                        };
                        let label = ui::galley(ui, name, font, ink, 2.0);
                        let at = row.left_center() + vec2(14.0, -label.size().y / 2.0);
                        ui.painter().galley(at, label, ink);
                        if response.clicked() {
                            self.section = section;
                            self.log_out_armed = None;
                        }
                    }
                });

                ui.allocate_ui_with_layout(ui.available_size(), Layout::top_down(Align::Min), |ui| {
                    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        match self.section {
                            Section::Appearance => self.appearance(ui, theme, config, &mut changes),
                            Section::Terminal => self.terminal(ui, theme, config, &mut changes),
                            Section::Frame => self.frame(ui, theme, panels, &mut changes),
                            Section::Network => {
                                let radio = self.radio.get_or_insert_with(|| Radio::new(ui.ctx().clone()));
                                radio.show_wifi(ui, theme);
                            }
                            Section::Bluetooth => {
                                let radio = self.radio.get_or_insert_with(|| Radio::new(ui.ctx().clone()));
                                radio.show_bluetooth(ui, theme);
                            }
                            Section::Keyboard => self.keyboard(ui, theme, input, &mut changes),
                            Section::Pointer => self.pointer(ui, theme, input, &mut changes),
                            Section::Displays => self.displays(ui, theme, displays, &mut changes),
                            Section::Globe => self.globe(ui, theme, config, geo, &mut changes),
                            Section::Sound => self.sound(ui, theme, mon, &mut changes),
                            Section::Session => self.session(ui, theme, &mut changes),
                        }
                    });
                });
            });
        });
        changes
    }

    fn appearance(&mut self, ui: &mut Ui, theme: &Theme, config: &Config, changes: &mut Vec<Change>) {
        heading(ui, "THEME", "Sets the accent colour and the terminal's colours. The greys stay.");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (name, accent) in &self.themes {
                let chosen = *name == config.theme;
                if ui::chip(ui, theme, &name.to_uppercase(), Some(*accent), chosen).clicked() && !chosen {
                    changes.push(Change::Theme(name.clone()));
                }
            }
        });
        ui.add_space(20.0);
        note(ui, "Fonts, text size and the cursor are under Terminal.");
    }

    fn terminal(&mut self, ui: &mut Ui, theme: &Theme, config: &Config, changes: &mut Vec<Change>) {
        heading(ui, "FONT", "The typeface the terminal is drawn in. Prompt symbols still come from your Nerd Font.");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let bundled = config.terminal_font.is_none();
            if ui::chip(ui, theme, "JETBRAINS MONO", None, bundled).clicked() && !bundled {
                changes.push(Change::Set("terminal_font", None));
            }
            for (family, file) in self.fonts.iter().filter(|(family, _)| family != "JetBrains Mono") {
                let chosen = config.terminal_font.as_deref() == Some(file.as_path());
                if ui::chip(ui, theme, &family.to_uppercase(), None, chosen).clicked() && !chosen {
                    changes.push(Change::Set("terminal_font", Some(file.to_string_lossy().into_owned())));
                }
            }
        });
        ui.add_space(22.0);

        heading(ui, "TEXT", "");
        let size_span = MAX_FONT_SIZE - MIN_FONT_SIZE;
        if let Some(frac) = slider_row(ui, theme, "Size", (config.font_size - MIN_FONT_SIZE) / size_span, &format!("{:.0} px", config.font_size)) {
            let size = (MIN_FONT_SIZE + frac * size_span).round();
            if size != config.font_size {
                changes.push(Change::FontSize(size));
            }
        }
        let (low, high) = LINE_HEIGHT_RANGE;
        if let Some(frac) = slider_row(ui, theme, "Line height", (config.line_height - low) / (high - low), &format!("{:.2}", config.line_height)) {
            let factor = ((low + frac * (high - low)) * 20.0).round() / 20.0;
            if factor != config.line_height {
                changes.push(Change::Set("line_height", Some(format!("{factor:.2}"))));
            }
        }
        if switch_row(ui, theme, "Bold text in bright colours", "As most terminals do. Off keeps bold text in its own colour", config.bold_bright) {
            changes.push(Change::Set("bold_bright", config.bold_bright.then(|| "off".to_string())));
        }
        ui.add_space(22.0);

        heading(ui, "CURSOR", "");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (shape, name, value) in [
                (CursorShape::Block, "BLOCK", None),
                (CursorShape::Bar, "BAR", Some("bar")),
                (CursorShape::Underline, "UNDERLINE", Some("underline")),
            ] {
                let chosen = config.cursor == shape;
                if ui::chip(ui, theme, name, None, chosen).clicked() && !chosen {
                    changes.push(Change::Set("cursor", value.map(str::to_string)));
                }
            }
        });
        ui.add_space(8.0);
        if switch_row(ui, theme, "Blink", "The cursor flashes once a second", config.cursor_blink) {
            changes.push(Change::Set("cursor_blink", (!config.cursor_blink).then(|| "on".to_string())));
        }
        ui.add_space(22.0);

        heading(ui, "BEHAVIOUR", "");
        if switch_row(ui, theme, "Copy on select", "Selected text goes to the clipboard at once, without Ctrl+Shift+C", config.copy_on_select) {
            changes.push(Change::Set("copy_on_select", (!config.copy_on_select).then(|| "on".to_string())));
        }
        ui.add_space(14.0);
        ui::text(ui, "Lines kept above the screen. Applies to tabs opened from now on.", ui::mono(12.0), ui::SECONDARY);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for lines in SCROLLBACK_CHOICES {
                let chosen = config.scrollback == lines;
                let label = if lines >= 1000 { format!("{} 000", lines / 1000) } else { lines.to_string() };
                if ui::chip(ui, theme, &label, None, chosen).clicked() && !chosen {
                    changes.push(Change::Set("scrollback", Some(lines.to_string())));
                }
            }
        });
    }

    fn keyboard(&mut self, ui: &mut Ui, theme: &Theme, input: &InputSettings, changes: &mut Vec<Change>) {
        heading(ui, "LAYOUT", "Which characters the keys produce.");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let default = input.keyboard_layout.is_empty();
            if ui::chip(ui, theme, "SYSTEM DEFAULT", None, default).clicked() && !default {
                changes.push(Change::Set("keyboard_layout", None));
                changes.push(Change::Set("keyboard_variant", None));
            }
            for (code, name) in LAYOUTS {
                let chosen = input.keyboard_layout == code;
                if ui::chip(ui, theme, name, None, chosen).clicked() && !chosen {
                    changes.push(Change::Set("keyboard_layout", Some(code.to_string())));
                    changes.push(Change::Set("keyboard_variant", None));
                }
            }
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui::text(ui, "Another, by its XKB name and optional variant", ui::mono(12.0), ui::SECONDARY);
            let (field, _) = ui.allocate_exact_size(vec2(200.0, 28.0), Sense::hover());
            ui.painter().rect_filled(field, 0.0, ui::PANEL);
            ui.painter().rect_stroke(field, 0.0, Stroke::new(1.0, ui::LINE_STRONG), StrokeKind::Inside);
            let edit = ui.put(
                field.shrink2(vec2(8.0, 4.0)),
                TextEdit::singleline(&mut self.layout_text)
                    .frame(false)
                    .font(ui::mono(13.0))
                    .text_color(ui::TEXT)
                    .hint_text("us dvorak"),
            );
            let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui::button(ui, theme, "USE", None, false).clicked() || entered) && !self.layout_text.trim().is_empty() {
                let mut words = self.layout_text.split_whitespace();
                changes.push(Change::Set("keyboard_layout", words.next().map(str::to_string)));
                changes.push(Change::Set("keyboard_variant", words.next().map(str::to_string)));
            }
        });
        if !input.keyboard_layout.is_empty() {
            ui.add_space(8.0);
            let variant = if input.keyboard_variant.is_empty() { String::new() } else { format!(", variant {}", input.keyboard_variant) };
            note(ui, &format!("In use: {}{variant}. A name the system does not know is ignored.", input.keyboard_layout));
        }
        ui.add_space(24.0);

        heading(ui, "KEY REPEAT", "What happens while a key is held down.");
        let (low, high) = REPEAT_DELAY_RANGE;
        let frac = (input.repeat_delay - low) as f32 / (high - low) as f32;
        if let Some(frac) = slider_row(ui, theme, "Delay before repeating", frac, &format!("{} ms", input.repeat_delay)) {
            let delay = (low as f32 + frac * (high - low) as f32) / 10.0;
            let delay = delay.round() as u32 * 10;
            if delay != input.repeat_delay {
                changes.push(Change::Set("keyboard_repeat_delay", Some(delay.to_string())));
            }
        }
        let (low, high) = REPEAT_RATE_RANGE;
        let frac = (input.repeat_rate - low) as f32 / (high - low) as f32;
        if let Some(frac) = slider_row(ui, theme, "Repeats per second", frac, &input.repeat_rate.to_string()) {
            let rate = (low as f32 + frac * (high - low) as f32).round() as u32;
            if rate != input.repeat_rate {
                changes.push(Change::Set("keyboard_repeat_rate", Some(rate.to_string())));
            }
        }
        ui.add_space(8.0);
        note(ui, "Keyboard changes reach applications as they are next given the keyboard.");
    }

    fn pointer(&mut self, ui: &mut Ui, theme: &Theme, input: &InputSettings, changes: &mut Vec<Change>) {
        let on_off = |on: bool| Some(if on { "on" } else { "off" }.to_string());
        let speed = |ui: &mut Ui, key: &'static str, value: f32, changes: &mut Vec<Change>| {
            let label = if value.abs() < 0.05 { "normal".to_string() } else { format!("{value:+.1}") };
            if let Some(frac) = slider_row(ui, theme, "Pointer speed", (value + 1.0) / 2.0, &label) {
                let new = ((frac * 2.0 - 1.0) * 10.0).round() / 10.0;
                if new != value {
                    changes.push(Change::Set(key, Some(format!("{new:.1}"))));
                }
            }
        };

        heading(ui, "TOUCHPAD", "");
        if switch_row(ui, theme, "Natural scrolling", "The page follows your fingers, as on a phone", input.touchpad_natural_scroll) {
            changes.push(Change::Set("touchpad_natural_scroll", on_off(!input.touchpad_natural_scroll)));
        }
        if switch_row(ui, theme, "Tap to click", "A light tap counts as a click; two fingers for a right click", input.touchpad_tap) {
            changes.push(Change::Set("touchpad_tap", on_off(!input.touchpad_tap)));
        }
        if switch_row(ui, theme, "Ignore while typing", "Stops a resting palm from moving the pointer", input.touchpad_while_typing_off) {
            changes.push(Change::Set("touchpad_while_typing_off", on_off(!input.touchpad_while_typing_off)));
        }
        ui.add_space(12.0);
        speed(ui, "touchpad_speed", input.touchpad_speed, changes);
        ui.add_space(22.0);

        heading(ui, "MOUSE", "");
        if switch_row(ui, theme, "Natural scrolling", "Reverses the wheel", input.mouse_natural_scroll) {
            changes.push(Change::Set("mouse_natural_scroll", on_off(!input.mouse_natural_scroll)));
        }
        if switch_row(ui, theme, "Left-handed", "Swaps the left and right buttons", input.mouse_left_handed) {
            changes.push(Change::Set("mouse_left_handed", on_off(!input.mouse_left_handed)));
        }
        ui.add_space(12.0);
        speed(ui, "mouse_speed", input.mouse_speed, changes);
    }

    fn frame(&mut self, ui: &mut Ui, theme: &Theme, panels: Panels, changes: &mut Vec<Change>) {
        heading(ui, "PANELS", "Which parts of the frame are open. The choice is kept for next time.");
        for (panel, open, name, what) in [
            (Panel::Left, panels.left, "Left column", "Clock, system, processor, memory, processes"),
            (Panel::Right, panels.right, "Right column", "Network, globe, displays, controls, keys"),
            (Panel::Bottom, panels.bottom, "File browser", "The folder the terminal is in"),
        ] {
            if switch_row(ui, theme, name, what, open) {
                changes.push(Change::Panel(panel, !open));
            }
        }
        ui.add_space(16.0);
        note(ui, "Super+[  Super+]  Super+\\ switch them at any time, without changing what is kept.");
    }

    fn displays(&mut self, ui: &mut Ui, theme: &Theme, displays: Option<&Displays>, changes: &mut Vec<Change>) {
        let Some(displays) = displays.filter(|displays| !displays.monitors.is_empty()) else {
            heading(ui, "DISPLAYS", "");
            note(ui, "Displays can be set when khadi runs as your login session.");
            return;
        };
        // Start over from what is real whenever that changes underneath the draft.
        if self.draft_basis != displays.monitors {
            self.draft = current_choices(&displays.monitors);
            self.draft_basis = displays.monitors.clone();
        }
        self.selected_display = self.selected_display.min(self.draft.len().saturating_sub(1));

        // A change being tried comes first: it is the one thing that cannot wait.
        if displays.pending > 0 {
            ui::tile(ui, theme, true, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let seconds = displays.pending;
                    let plural = if seconds == 1 { "" } else { "S" };
                    ui::tracked(ui, &format!("KEEP THIS LAYOUT?  GOING BACK IN {seconds} SECOND{plural}"), ui::display_bold(13.0), ui::TEXT, 1.5);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui::button(ui, theme, "GO BACK", None, false).clicked() {
                            changes.push(Change::RevertDisplays);
                        }
                        if ui::button(ui, theme, "KEEP", None, true).clicked() {
                            changes.push(Change::KeepDisplays);
                        }
                    });
                });
            });
            ui.add_space(18.0);
        }

        heading(ui, "ARRANGEMENT", "As they stand on your desk, left to right. Click one to change it.");
        // The monitors to scale, in a row.
        let total: f32 = self.draft.iter().map(|choice| choice.mode.width as f32).sum();
        let tallest = self.draft.iter().map(|choice| choice.mode.height).max().unwrap_or(1) as f32;
        let scale = ((ui.available_width() - 8.0 * self.draft.len() as f32) / total.max(1.0)).min(130.0 / tallest);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (index, choice) in self.draft.iter().enumerate() {
                let size = vec2(choice.mode.width as f32 * scale, choice.mode.height as f32 * scale);
                let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                let chosen = index == self.selected_display;
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 0.0, if chosen { theme.tint() } else { ui::PANEL });
                let line = if chosen { theme.main } else if response.hovered() { ui::SECONDARY } else { ui::LINE_STRONG };
                painter.rect_stroke(rect, 0.0, Stroke::new(if chosen { 2.0 } else { 1.0 }, line), StrokeKind::Inside);
                painter.text(rect.left_top() + vec2(10.0, 8.0), Align2::LEFT_TOP, &choice.name, ui::mono(12.0), ui::TEXT);
                let detail = format!("{} × {}", choice.mode.width, choice.mode.height);
                painter.text(rect.left_top() + vec2(10.0, 26.0), Align2::LEFT_TOP, detail, ui::mono(11.0), ui::SECONDARY);
                if choice.main {
                    let label = ui::galley(ui, "MAIN", ui::display_bold(11.0), theme.main, 2.0);
                    painter.galley(rect.left_bottom() + vec2(10.0, -8.0 - label.size().y), label, theme.main);
                }
                if response.clicked() {
                    self.selected_display = index;
                }
            }
        });
        ui.add_space(14.0);

        let selected = self.selected_display;
        let Some(monitor) = displays.monitors.iter().find(|monitor| monitor.name == self.draft[selected].name) else {
            return;
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if self.draft.len() > 1 {
                if selected > 0 && ui::button(ui, theme, "MOVE LEFT", None, false).clicked() {
                    self.draft.swap(selected, selected - 1);
                    self.selected_display = selected - 1;
                }
                if selected + 1 < self.draft.len() && ui::button(ui, theme, "MOVE RIGHT", None, false).clicked() {
                    self.draft.swap(selected, selected + 1);
                    self.selected_display = selected + 1;
                }
                let is_main = self.draft[self.selected_display].main;
                let label = if is_main { "MAIN DISPLAY" } else { "MAKE MAIN" };
                if ui::button(ui, theme, label, None, is_main).clicked() && !is_main {
                    let chosen = self.selected_display;
                    for (index, choice) in self.draft.iter_mut().enumerate() {
                        choice.main = index == chosen;
                    }
                }
            }
        });
        if self.draft.len() > 1 {
            ui.add_space(6.0);
            note(ui, "The main display carries the frame and the terminal.");
        }
        ui.add_space(24.0);

        let selected = self.selected_display;
        heading(ui, "RESOLUTION", &format!("For {}.", monitor.name));
        let mode = self.draft[selected].mode;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for size in sizes(monitor).into_iter().take(14) {
                let chosen = (mode.width, mode.height) == size;
                let label = format!("{} × {}", size.0, size.1);
                if ui::chip(ui, theme, &label, None, chosen).clicked() && !chosen {
                    if let Some(best) = best_mode(monitor, size) {
                        self.draft[selected].mode = best;
                    }
                }
            }
        });
        ui.add_space(22.0);

        heading(ui, "REFRESH RATE", "");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            let rates = monitor.modes.iter().filter(|other| (other.width, other.height) == (mode.width, mode.height));
            for rate in rates {
                let chosen = rate.refresh == mode.refresh;
                if ui::chip(ui, theme, &hertz(rate.refresh), None, chosen).clicked() {
                    self.draft[selected].mode = *rate;
                }
            }
        });
        ui.add_space(26.0);

        let changed = self.draft != current_choices(&displays.monitors);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if changed {
                if ui::button(ui, theme, "APPLY", None, true).clicked() {
                    changes.push(Change::ApplyDisplays(self.draft.clone()));
                }
                if ui::button(ui, theme, "DISCARD", None, false).clicked() {
                    self.draft = current_choices(&displays.monitors);
                }
            }
        });
        ui.add_space(8.0);
        note(ui, if changed {
            "Applying tries the layout for 15 seconds. If you cannot see it to confirm, it goes back by itself."
        } else {
            "This is how the displays are set now."
        });
    }

    fn globe(&mut self, ui: &mut Ui, theme: &Theme, config: &Config, geo: &Snapshot, changes: &mut Vec<Change>) {
        heading(ui, "GLOBE", "");
        if switch_row(ui, theme, "Rotation", "Off holds the globe still on your side of the world, and saves power", config.globe_rotation) {
            changes.push(Change::GlobeRotation(!config.globe_rotation));
        }
        if switch_row(
            ui,
            theme,
            "Look up places online",
            "Asks get.geojs.io where addresses are. It learns which public addresses this machine connects to",
            config.lookups,
        ) {
            changes.push(Change::Lookups(!config.lookups));
        }
        ui.add_space(26.0);

        heading(ui, "YOUR LOCATION", "Latitude, longitude and a name, such as  48.8566, 2.3522, Paris");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (field, _) = ui.allocate_exact_size(vec2(360.0, 36.0), Sense::hover());
            let line = if self.location_error { ui::CAUTION } else { ui::LINE_STRONG };
            ui.painter().rect_filled(field, 0.0, ui::PANEL);
            ui.painter().rect_stroke(field, 0.0, Stroke::new(1.0, line), StrokeKind::Inside);
            let edit = ui.put(
                field.shrink2(vec2(10.0, 7.0)),
                TextEdit::singleline(&mut self.location_text)
                    .frame(false)
                    .font(ui::mono(13.0))
                    .text_color(ui::TEXT)
                    .hint_text("latitude, longitude, name"),
            );
            let entered = edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
            if ui::button(ui, theme, "SET", None, false).clicked() || entered {
                match parse_location(&self.location_text) {
                    Some(place) => {
                        self.location_error = false;
                        changes.push(Change::Location(Some(place)));
                    }
                    None => self.location_error = true,
                }
            }
            if config.location.is_some() && ui::button(ui, theme, "GUESS FROM MY ADDRESS", None, false).clicked() {
                self.location_text.clear();
                self.location_error = false;
                changes.push(Change::Location(None));
            }
        });
        ui.add_space(10.0);
        if self.location_error {
            ui::text(ui, "That is not a position: latitude is -90 to 90, longitude -180 to 180.", ui::mono(12.0), ui::CAUTION);
        } else {
            let now = match (&geo.home, geo.home_is_set) {
                (Some(home), true) => format!("Shown at {}, as you set it.", home.label),
                (Some(home), false) => format!("Shown at {}, guessed from your address.", home.label),
                (None, _) if !config.lookups => "Not shown: lookups are off and no position is set.".to_string(),
                (None, _) => "Not known yet.".to_string(),
            };
            note(ui, &now);
        }
    }

    fn sound(&mut self, ui: &mut Ui, theme: &Theme, mon: &SysMon, changes: &mut Vec<Change>) {
        heading(ui, "SOUND & SCREEN", "The volume and brightness keys do the same.");
        let volume = held(&mut self.volume_held, mon.volume());
        let brightness = held(&mut self.brightness_held, mon.brightness);
        let level = |ui: &mut Ui, name: &str, value: Option<u8>| -> Option<u8> {
            let mut picked = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let (label, _) = ui.allocate_exact_size(vec2(110.0, 20.0), Sense::hover());
                ui.painter().text(label.left_center(), Align2::LEFT_CENTER, name, ui::mono(13.0), ui::TEXT);
                match value {
                    Some(value) => {
                        if let Some(frac) = ui::slider(ui, theme, 300.0, value.min(100) as f32 / 100.0) {
                            let new = (frac * 100.0).round() as u8;
                            if new != value {
                                picked = Some(new);
                            }
                        }
                        ui::text(ui, &format!("{value}"), ui::mono(12.0), ui::TEXT);
                    }
                    None => {
                        ui::text(ui, "not available on this machine", ui::mono(12.0), ui::LABEL);
                    }
                }
            });
            ui.add_space(14.0);
            picked
        };
        if let Some(new) = level(ui, "Volume", volume) {
            self.volume_held = Some((new, Instant::now()));
            changes.push(Change::Volume(new));
        }
        if let Some(new) = level(ui, "Brightness", brightness) {
            // Never all the way down: on many panels that is a black screen.
            let new = new.max(5);
            self.brightness_held = Some((new, Instant::now()));
            changes.push(Change::Brightness(new));
        }
        if let Some(battery) = mon.battery {
            note(ui, &format!("Battery at {battery}%."));
        }
    }

    fn session(&mut self, ui: &mut Ui, theme: &Theme, changes: &mut Vec<Change>) {
        heading(ui, "SESSION", "");
        for (name, value) in [
            ("Version", env!("CARGO_PKG_VERSION")),
            ("Settings file", "~/.config/khadi/config"),
            ("Session log", "~/.local/state/khadi/session.log"),
            ("Application output", "~/.local/state/khadi/apps.log"),
        ] {
            ui.horizontal(|ui| {
                let (label, _) = ui.allocate_exact_size(vec2(180.0, 26.0), Sense::hover());
                ui.painter().text(label.left_center(), Align2::LEFT_CENTER, name, ui::mono(12.0), ui::SECONDARY);
                ui::text(ui, value, ui::mono(12.0), ui::TEXT);
            });
        }
        ui.add_space(22.0);
        note(ui, "Super+Shift+R restarts the shell on a newly installed build. Applications stay open; terminal tabs close.");
        ui.add_space(22.0);

        let armed = self.log_out_armed.is_some_and(|at| at.elapsed() < Duration::from_secs(5));
        let label = if armed { "CLICK AGAIN TO LOG OUT" } else { "LOG OUT" };
        if ui::button(ui, theme, label, None, armed).clicked() {
            if armed {
                changes.push(Change::LogOut);
            } else {
                self.log_out_armed = Some(Instant::now());
            }
        }
        if !armed {
            self.log_out_armed = None;
        }
        ui.add_space(8.0);
        note(ui, "Logging out closes every application and terminal tab.");
    }
}

/// Keyboard layouts offered as one click: XKB name, and what to call it.
const LAYOUTS: [(&str, &str); 14] = [
    ("us", "ENGLISH (US)"),
    ("gb", "ENGLISH (UK)"),
    ("fr", "FRENCH"),
    ("de", "GERMAN"),
    ("es", "SPANISH"),
    ("pt", "PORTUGUESE"),
    ("br", "PORTUGUESE (BRAZIL)"),
    ("it", "ITALIAN"),
    ("nl", "DUTCH"),
    ("se", "SWEDISH"),
    ("pl", "POLISH"),
    ("tr", "TURKISH"),
    ("ru", "RUSSIAN"),
    ("ara", "ARABIC"),
];

/// A named slider with its reading. Returns the new position while it is moved.
fn slider_row(ui: &mut Ui, theme: &Theme, name: &str, frac: f32, reading: &str) -> Option<f32> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let (label, _) = ui.allocate_exact_size(vec2(190.0, 20.0), Sense::hover());
        ui.painter().text(label.left_center(), Align2::LEFT_CENTER, name, ui::mono(13.0), ui::TEXT);
        // Narrower on a narrow workspace, so that the reading still fits beside it.
        let width = (ui.available_width() - 90.0).clamp(100.0, 280.0);
        picked = ui::slider(ui, theme, width, frac);
        ui::text(ui, reading, ui::mono(12.0), ui::TEXT);
    });
    ui.add_space(12.0);
    picked
}

/// A group's title and, under it, what the group is for.
fn heading(ui: &mut Ui, title: &str, about: &str) {
    ui::section(ui, title, |_| {});
    if !about.is_empty() {
        ui::text(ui, about, ui::mono(12.0), ui::SECONDARY);
    }
    ui.add_space(14.0);
}

fn note(ui: &mut Ui, text: &str) {
    ui::text(ui, text, ui::mono(12.0), ui::LABEL);
}

/// A named switch with a line of explanation. Returns true when it was clicked.
fn switch_row(ui: &mut Ui, theme: &Theme, name: &str, what: &str, on: bool) -> bool {
    let mut clicked = false;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
    let mut inner = ui.new_child(UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)));
    inner.allocate_ui_with_layout(vec2((row.width() - 70.0).max(40.0), 40.0), Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui::text(ui, name, ui::mono(13.0), ui::TEXT);
        ui::text(ui, what, ui::mono(11.0), ui::LABEL);
    });
    inner.with_layout(Layout::right_to_left(Align::Center), |ui| {
        clicked = ui::toggle(ui, theme, on).clicked();
    });
    ui.painter().hline(row.x_range(), row.bottom(), Stroke::new(1.0, ui::TRACK));
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_what_a_monitor_offers() {
        let mode = |width, height, refresh| Mode { width, height, refresh };
        let monitor = DisplayInfo {
            name: "HDMI-A-1".into(),
            main: false,
            x: 0,
            y: 0,
            current: mode(1920, 1080, 60000),
            modes: vec![mode(3840, 2160, 30000), mode(1920, 1080, 60000), mode(1920, 1080, 59940), mode(1280, 720, 60000)],
        };
        assert_eq!(sizes(&monitor), [(3840, 2160), (1920, 1080), (1280, 720)]);
        assert_eq!(best_mode(&monitor, (1920, 1080)), Some(mode(1920, 1080, 60000)));
        assert_eq!(best_mode(&monitor, (800, 600)), None);
        assert_eq!(hertz(60000), "60 Hz");
        assert_eq!(hertz(59940), "59.94 Hz");
        assert_eq!(hertz(60052), "60.05 Hz");
        let choices = current_choices(std::slice::from_ref(&monitor));
        assert_eq!(choices[0].mode, monitor.current);
    }

    #[test]
    fn a_level_just_set_is_held_then_follows_the_system() {
        let mut slot = Some((40, Instant::now()));
        assert_eq!(held(&mut slot, Some(80)), Some(40));
        let mut slot = Some((40, Instant::now() - Duration::from_secs(10)));
        assert_eq!(held(&mut slot, Some(80)), Some(80));
        assert_eq!(slot, None);
        assert_eq!(held(&mut None, None), None);
    }
}
