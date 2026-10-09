//! The frame around the workspace: the two monitoring columns, the file browser, the
//! status strip, and what a further display shows while it is idle.

use crate::geo::Snapshot;
use crate::globe;
use crate::sysmon::SysMon;
use crate::theme::Theme;
use crate::ui::{self, Icon};
use khadi_common::{
    Panel, Panels,
    ipc::{DesktopState, Place},
};
use eframe::egui::{
    Align, Align2, Layout, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// Space between a column's sections.
const SECTION_GAP: f32 = 22.0;

fn human_bytes(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// Lays two equal tiles side by side.
fn tile_pair(ui: &mut Ui, mut left: impl FnMut(&mut Ui), mut right: impl FnMut(&mut Ui)) {
    let width = (ui.available_width() - 8.0) / 2.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| left(ui));
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| right(ui));
    });
}

/// The clock, in the display face: hours and minutes large, seconds in the accent.
fn clock(ui: &mut Ui, theme: &Theme, size: f32) {
    let now = chrono::Local::now();
    let main = ui::galley(ui, &now.format("%H:%M").to_string(), ui::display(size), ui::TEXT, 1.0);
    let seconds = ui::galley(ui, &now.format("%S").to_string(), ui::display(size * 0.35), theme.main, 0.0);
    let width = main.size().x + size * 0.12 + seconds.size().x;
    let height = main.size().y * 0.84;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let baseline = rect.bottom();
    let seconds_pos = pos2(rect.left() + main.size().x + size * 0.12, baseline - seconds.size().y * 0.95);
    ui.painter().galley(pos2(rect.left(), baseline - main.size().y * 0.92), main, ui::TEXT);
    ui.painter().galley(seconds_pos, seconds, theme.main);
}

pub fn left(ui: &mut Ui, theme: &Theme, mon: &SysMon) {
    ui.spacing_mut().item_spacing.y = 7.0;
    let now = chrono::Local::now();
    // The clock is sized for a full-width column and shrinks with it.
    clock(ui, theme, 68.0 * (ui.available_width() / 272.0).min(1.0));
    ui.horizontal(|ui| {
        let date = now.format("%a %d %b %Y").to_string().to_uppercase();
        let date = ui::tracked(ui, &date, ui::display(13.0), ui::SECONDARY, 2.0);
        let up = SysMon::uptime();
        let uptime = format!("UP {}D {}H", up / 86400, (up / 3600) % 24);
        let uptime = ui::galley(ui, &uptime, ui::display(13.0), ui::SECONDARY, 2.0);
        // On a narrow column the date alone has to do.
        let column = ui.max_rect();
        if date.rect.right() + 16.0 + uptime.size().x <= column.right() {
            let at = pos2(column.right() - uptime.size().x, date.rect.top());
            ui.painter().galley(at, uptime, ui::SECONDARY);
        }
    });

    ui.add_space(SECTION_GAP);
    ui::section(ui, "SYSTEM", |ui| ui::meta(ui, &truncate(&mon.host, 18), ui::LABEL));
    tile_pair(
        ui,
        |ui| ui::value_tile(ui, theme, "OS", &truncate(&mon.os, 14)),
        |ui| ui::value_tile(ui, theme, "KERNEL", &truncate(&mon.kernel, 14)),
    );

    ui.add_space(SECTION_GAP);
    let load = mon.cpu_hist.back().copied().unwrap_or(0.0);
    ui::section(ui, "PROCESSOR", |ui| ui::meta(ui, &format!("{load:.0}%"), ui::TEXT));
    ui::sparkline(ui, &mon.cpu_hist, 100.0, vec2(ui.available_width(), 56.0), theme.main, true);
    ui.add_space(3.0);
    // Past eight cores the list would crowd out everything below it.
    for (i, usage) in mon.cores.iter().take(8).enumerate() {
        ui::meter(ui, theme, &format!("C{}", i + 1), 22.0, usage / 100.0, &format!("{usage:.0}"));
    }

    ui.add_space(SECTION_GAP);
    ui::section(ui, "MEMORY", |ui| {
        let total = format!("/ {:.1} GiB", mon.mem_total as f64 / GIB);
        ui::meta(ui, &total, ui::LABEL);
        ui::meta(ui, &format!("{:.1}", mon.mem_used as f64 / GIB), ui::TEXT);
    });
    ui::bar(ui, theme, 10.0, mon.mem_used as f32 / mon.mem_total.max(1) as f32, 0.0);
    ui.horizontal(|ui| {
        ui::text(ui, "USED", ui::mono(11.0), ui::SECONDARY);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let swap = format!("SWAP {:.1} GiB", mon.swap_used as f64 / GIB);
            ui::text(ui, &swap, ui::mono(11.0), ui::SECONDARY);
        });
    });

    ui.add_space(SECTION_GAP);
    ui::section(ui, "PROCESSES", |ui| ui::meta(ui, "CPU · MEM", ui::LABEL));
    ui.spacing_mut().item_spacing.y = 0.0;
    for process in &mon.top {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 27.0), Sense::hover());
        if rect.bottom() > ui.max_rect().bottom() {
            break;
        }
        let painter = ui.painter();
        let busy = process.cpu > ui::CAUTION_ABOVE * 100.0 / 2.0;
        painter.text(rect.left_center(), Align2::LEFT_CENTER, truncate(&process.name, 16), ui::mono(12.0), ui::TEXT);
        painter.text(
            rect.right_center() - vec2(70.0, 0.0),
            Align2::RIGHT_CENTER,
            format!("{:.0}%", process.cpu),
            ui::mono(12.0),
            if busy { ui::CAUTION } else { ui::TEXT },
        );
        painter.text(rect.right_center(), Align2::RIGHT_CENTER, human_bytes(process.mem as f64), ui::mono(12.0), ui::SECONDARY);
        painter.hline(rect.x_range(), rect.bottom(), Stroke::new(1.0, ui::TRACK));
    }
}

/// What each display is showing, for the right column.
fn display_tiles(ui: &mut Ui, theme: &Theme, desktop: &DesktopState) {
    let describe = |screen: usize| {
        let shown: Vec<&str> = desktop
            .apps
            .iter()
            .filter(|app| app.screen == screen && app.place != Place::Hidden)
            .map(|app| app.title.as_str())
            .collect();
        match (shown.as_slice(), screen) {
            ([], 0) => "Terminal".to_string(),
            ([], _) => "Idle".to_string(),
            ([one], 0) if desktop.terminal_split => format!("Terminal | {one}"),
            (titles, _) => titles.join(" | "),
        }
    };
    let focused_screen = match desktop.focus {
        Some(index) => desktop.apps.get(index).map_or(0, |app| app.screen),
        None => 0,
    };
    let draw = |ui: &mut Ui, screen: usize| {
        let active = screen == focused_screen;
        ui::tile(ui, theme, active, |ui| {
            let label = if screen == 0 {
                "1 · MAIN".to_string()
            } else {
                format!("{}", screen + 1)
            };
            ui::tracked(ui, &label, ui::display(10.0), if active { theme.main } else { ui::LABEL }, 2.0);
            ui::text(ui, &truncate(&describe(screen), 15), ui::mono(12.0), ui::TEXT);
        });
    };
    for pair in (0..desktop.screens).collect::<Vec<_>>().chunks(2) {
        match pair {
            [a, b] => {
                let (a, b) = (*a, *b);
                let width = (ui.available_width() - 8.0) / 2.0;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| draw(ui, a));
                    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| draw(ui, b));
                });
            }
            [a] => draw(ui, *a),
            _ => {}
        }
    }
}

/// The right column. `rotation` turns the globe; `desktop` is absent when the shell
/// runs as an ordinary window.
pub fn right(
    ui: &mut Ui,
    theme: &Theme,
    mon: &SysMon,
    desktop: Option<&DesktopState>,
    rotation: f32,
    geo: &Snapshot,
) {
    ui.spacing_mut().item_spacing.y = 7.0;
    let online = !mon.ip.is_empty();
    ui::section(ui, "NETWORK", |ui| {
        let (label, color) = if online {
            ("ONLINE", theme.main)
        } else {
            ("OFFLINE", ui::CAUTION)
        };
        ui::meta(ui, label, color);
        let (dot, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
        ui.painter().rect_filled(dot, 0.0, color);
    });
    ui.horizontal(|ui| {
        let iface = if mon.iface.is_empty() { "no interface" } else { &mon.iface };
        ui::text(ui, iface, ui::mono(12.0), ui::LABEL);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui::text(ui, if online { &mon.ip } else { "—" }, ui::mono(12.0), ui::TEXT);
        });
    });
    // Both graphs share a scale so their heights are comparable.
    let peak = mon
        .rx_hist
        .iter()
        .chain(mon.tx_hist.iter())
        .fold(1024.0_f32, |a, b| a.max(*b));
    let traffic = |ui: &mut Ui, label: &str, total: u64, hist: &std::collections::VecDeque<f32>, color| {
        ui::tile(ui, theme, false, |ui| {
            ui.horizontal(|ui| {
                ui::tracked(ui, label, ui::display(10.0), ui::LABEL, 2.0);
                // The running total, where the tile is wide enough for both.
                if ui.available_width() > 78.0 {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui::tracked(ui, &human_bytes(total as f64), ui::display(10.0), ui::LABEL, 1.0);
                    });
                }
            });
            let rate = hist.back().copied().unwrap_or(0.0);
            ui::text(ui, &format!("{}/s", human_bytes(rate as f64)), ui::mono(14.0), ui::TEXT);
            ui::sparkline(ui, hist, peak, vec2(ui.available_width(), 26.0), color, false);
        });
    };
    let width = (ui.available_width() - 8.0) / 2.0;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            traffic(ui, "DOWN", mon.rx_total, &mon.rx_hist, theme.main)
        });
        ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            traffic(ui, "UP", mon.tx_total, &mon.tx_hist, ui::SECONDARY)
        });
    });

    ui.add_space(SECTION_GAP);
    ui::section(ui, "WORLD VIEW", |ui| {
        if !geo.links.is_empty() {
            let plural = if geo.links.len() == 1 { "" } else { "S" };
            ui::meta(ui, &format!("{} LINK{plural}", geo.links.len()), ui::LABEL);
        }
    });
    globe::show(ui, theme, (ui.available_width() * 0.82).min(224.0), rotation, geo);
    if let Some(home) = &geo.home {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (dot, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
            ui.painter().rect_filled(dot, 0.0, ui::TEXT);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Says how this position is known, since one of the ways is a guess.
                ui::meta(ui, if geo.home_is_set { "SET BY YOU" } else { "BY IP" }, ui::LABEL);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui::text(ui, &home.label.to_uppercase(), ui::mono(11.0), ui::SECONDARY);
                });
            });
        });
    }

    if let Some(desktop) = desktop {
        ui.add_space(SECTION_GAP);
        ui::section(ui, "DISPLAYS", |ui| {
            ui::meta(ui, &format!("{} CONNECTED", desktop.screens), ui::LABEL)
        });
        display_tiles(ui, theme, desktop);
    }

    let readings = [
        ("VOLUME", mon.volume()),
        ("BRIGHTNESS", mon.brightness),
        ("BATTERY", mon.battery),
    ];
    if readings.iter().any(|(_, value)| value.is_some()) {
        ui.add_space(SECTION_GAP);
        ui::section(ui, "CONTROLS", |_| {});
        for (label, value) in readings {
            if let Some(value) = value {
                // These are levels, not loads: a full battery is not a warning.
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
                let painter = ui.painter();
                painter.text(rect.left_center(), Align2::LEFT_CENTER, label, ui::mono(12.0), ui::LABEL);
                painter.text(rect.right_center(), Align2::RIGHT_CENTER, value.to_string(), ui::mono(12.0), ui::TEXT);
                let track = Rect::from_min_max(
                    pos2(rect.left() + 88.0, rect.center().y - 3.0),
                    pos2(rect.right() - 40.0, rect.center().y + 3.0),
                );
                painter.rect_filled(track, 0.0, ui::TRACK);
                let mut filled = track;
                filled.set_width(track.width() * (value as f32 / 100.0).min(1.0));
                painter.rect_filled(filled, 0.0, theme.main);
            }
        }
    }

    ui.add_space(SECTION_GAP);
    ui::section(ui, "KEYS", |ui| ui::meta(ui, "HOLD SUPER", ui::LABEL));
    for (action, key) in [
        ("Launcher", "SPACE"),
        ("Settings", ","),
        ("Next application", "TAB"),
        ("Split the workspace", "S"),
        ("Full screen", "F"),
        ("Other display", "O"),
        ("Close application", "Q"),
        ("Panels", "[  ]  \\"),
        ("Restart shell", "SHIFT R"),
    ] {
        // Only as many as fit: the list is a reminder, not a reference.
        if ui.available_height() < 26.0 {
            break;
        }
        ui::key_row(ui, action, key);
    }
}

/// The strip along the bottom edge: what has the keyboard, the panel switches, and
/// the readings worth seeing at all times. Returns a panel whose switch was clicked,
/// and whether the settings were asked for. `overlay` names what is shown in place of
/// the terminal, if anything.
pub fn status(
    ui: &mut Ui,
    theme: &Theme,
    mon: &SysMon,
    desktop: Option<&DesktopState>,
    panels: Panels,
    overlay: Option<&str>,
) -> (Option<Panel>, bool) {
    let mut toggled = None;
    ui.spacing_mut().item_spacing.x = 20.0;
    ui::tracked(ui, "Khadi", ui::display_bold(11.0), theme.main, 3.0);
    // The way into the settings, lit while they are open.
    let in_settings = overlay == Some("SETTINGS");
    let label = ui::galley(ui, "SETTINGS", ui::display(11.0), ui::SECONDARY, 2.0);
    let (rect, response) = ui.allocate_exact_size(label.size() + vec2(0.0, 8.0), Sense::click());
    let ink = if in_settings {
        theme.main
    } else if response.hovered() {
        ui::TEXT
    } else {
        ui::SECONDARY
    };
    ui.painter().galley(rect.left_center() - vec2(0.0, label.size().y / 2.0), label, ink);
    let settings_clicked = response.on_hover_text("Settings (Super+,)").clicked();

    let holder = match (overlay, desktop.and_then(|d| d.focus.and_then(|index| d.apps.get(index)))) {
        (Some(overlay), _) => overlay.to_string(),
        (None, Some(app)) => truncate(&app.title, 28).to_uppercase(),
        (None, None) => "TERMINAL".to_string(),
    };
    ui::text(ui, &format!("{holder} HAS THE KEYBOARD"), ui::mono(11.0), ui::SECONDARY);

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if let Some(battery) = mon.battery {
            ui::text(ui, &format!("BAT {battery}%"), ui::mono(11.0), ui::TEXT);
        }
        if let Some(volume) = mon.volume() {
            ui::text(ui, &format!("VOL {volume}"), ui::mono(11.0), ui::SECONDARY);
        }
        if let Some(wifi) = mon.wifi {
            ui::text(ui, &format!("WIFI {wifi}%"), ui::mono(11.0), ui::SECONDARY);
        }
        if let Some(desktop) = desktop {
            if desktop.screens > 1 {
                ui::text(ui, &format!("{} DISPLAYS", desktop.screens), ui::mono(11.0), ui::SECONDARY);
            }
            let count = desktop.apps.len();
            let plural = if count == 1 { "" } else { "S" };
            ui::text(ui, &format!("{count} APPLICATION{plural}"), ui::mono(11.0), ui::SECONDARY);
        }
        ui.spacing_mut().item_spacing.x = 4.0;
        for (icon, panel, open) in [
            (Icon::PanelRight, Panel::Right, panels.right),
            (Icon::PanelBottom, Panel::Bottom, panels.bottom),
            (Icon::PanelLeft, Panel::Left, panels.left),
        ] {
            let name = match panel {
                Panel::Left => "left",
                Panel::Right => "right",
                Panel::Bottom => "bottom",
            };
            let action = if open { "Collapse" } else { "Open" };
            let button = ui::icon_button(ui, theme, icon, 22.0, open)
                .on_hover_text(format!("{action} the {name} panel"));
            if button.clicked() {
                toggled = Some(panel);
            }
        }
    });
    (toggled, settings_clicked)
}

/// What a further display shows while no application is on it.
pub fn backdrop(ui: &mut Ui, theme: &Theme, mon: &SysMon, number: usize) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, ui::GROUND);
    ui::corners(ui.painter(), rect.shrink(24.0), 28.0, Stroke::new(2.0, ui::LINE_STRONG));

    let inner = rect.shrink2(vec2(72.0, 48.0));
    let painter = ui.painter().clone();
    let brand = ui::galley(ui, "Khadi", ui::display_bold(14.0), theme.main, 4.0);
    let brand_width = brand.size().x;
    painter.galley(inner.left_top(), brand, theme.main);
    let label = ui::galley(ui, &format!("DISPLAY {number}"), ui::display(14.0), ui::SECONDARY, 4.0);
    painter.galley(inner.left_top() + vec2(brand_width + 14.0, 0.0), label, ui::SECONDARY);
    painter.text(
        inner.right_top(),
        Align2::RIGHT_TOP,
        format!("{:.0} × {:.0}", rect.width(), rect.height()),
        ui::mono(12.0),
        ui::SECONDARY,
    );

    // The clock takes about a third of the height, and never more than the width allows.
    let now = chrono::Local::now();
    let size = (rect.height() * 0.2).min(rect.width() * 0.16);
    let time = ui::galley(ui, &now.format("%H:%M").to_string(), ui::display(size), ui::TEXT, 4.0);
    let seconds = ui::galley(ui, &now.format("%S").to_string(), ui::display(size * 0.3), theme.main, 0.0);
    let total = time.size().x + size * 0.08 + seconds.size().x;
    let top = rect.center().y - time.size().y * 0.62;
    let left = rect.center().x - total / 2.0;
    let seconds_pos = pos2(left + time.size().x + size * 0.08, top + time.size().y * 0.86 - seconds.size().y);
    let time_bottom = top + time.size().y;
    painter.galley(pos2(left, top), time, ui::TEXT);
    painter.galley(seconds_pos, seconds, theme.main);
    let date = now.format("%A %d %B %Y").to_string().to_uppercase();
    let date = ui::galley(ui, &date, ui::display(size * 0.11), ui::SECONDARY, size * 0.035);
    let date_height = date.size().y;
    painter.galley(pos2(rect.center().x - date.size().x / 2.0, time_bottom), date, ui::SECONDARY);
    let hint_y = time_bottom + date_height + 40.0;
    painter.hline(
        (rect.center().x - 260.0)..=(rect.center().x + 260.0),
        hint_y - 20.0,
        Stroke::new(1.0, ui::LINE),
    );
    // One line where the display is wide enough for it, two where it is not.
    let hints = ["SUPER O  sends the current application here", "SUPER SPACE  opens the launcher"];
    if rect.width() >= 1100.0 {
        painter.text(pos2(rect.center().x, hint_y), Align2::CENTER_TOP, hints.join("      "), ui::mono(13.0), ui::SECONDARY);
    } else {
        for (row, hint) in hints.iter().enumerate() {
            let at = pos2(rect.center().x, hint_y + row as f32 * 22.0);
            painter.text(at, Align2::CENTER_TOP, hint, ui::mono(13.0), ui::SECONDARY);
        }
    }

    // Four readings along the bottom.
    let load = mon.cpu_hist.back().copied().unwrap_or(0.0);
    let memory = mon.mem_used as f32 / mon.mem_total.max(1) as f32;
    let down = mon.rx_hist.back().copied().unwrap_or(0.0);
    let peak = mon.rx_hist.iter().fold(1024.0_f32, |a, b| a.max(*b));
    let tiles = [
        ("PROCESSOR", format!("{load:.0}%"), load / 100.0, true),
        ("MEMORY", format!("{:.1} GiB", mon.mem_used as f64 / GIB), memory, true),
        ("NETWORK", format!("{}/s", human_bytes(down as f64)), down / peak, false),
        (
            "BATTERY",
            mon.battery.map_or("AC".to_string(), |b| format!("{b}%")),
            mon.battery.map_or(1.0, |b| b as f32 / 100.0),
            false,
        ),
    ];
    let gap = 16.0;
    let width = (inner.width() - gap * 3.0) / 4.0;
    for (i, (label, reading, frac, is_load)) in tiles.into_iter().enumerate() {
        let tile = Rect::from_min_size(
            pos2(inner.left() + i as f32 * (width + gap), inner.bottom() - 58.0),
            vec2(width, 58.0),
        );
        painter.rect_filled(tile, 0.0, ui::PANEL);
        painter.rect_stroke(tile, 0.0, Stroke::new(1.0, ui::LINE), StrokeKind::Inside);
        let name = ui::galley(ui, label, ui::display_bold(12.0), ui::SECONDARY, 3.0);
        painter.galley(tile.left_top() + vec2(16.0, 13.0), name, ui::SECONDARY);
        if width >= 200.0 {
            painter.text(tile.right_top() + vec2(-16.0, 13.0), Align2::RIGHT_TOP, reading, ui::mono(12.0), ui::TEXT);
        }
        let track = Rect::from_min_size(tile.left_top() + vec2(16.0, 38.0), vec2(width - 32.0, 6.0));
        painter.rect_filled(track, 0.0, ui::TRACK);
        let frac = frac.clamp(0.0, 1.0);
        let mut filled = track;
        filled.set_width(track.width() * frac);
        let color = if is_load && frac > ui::CAUTION_ABOVE { ui::CAUTION } else { theme.main };
        painter.rect_filled(filled, 0.0, color);
    }
}

/// Quotes `s` so a POSIX shell reads it back as a single word.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A directory listing that follows the terminal's working directory.
pub struct FsView {
    path: PathBuf,
    entries: Vec<(String, bool)>,
    loaded: Option<Instant>,
}

impl FsView {
    pub fn new() -> FsView {
        FsView {
            path: PathBuf::new(),
            entries: Vec::new(),
            loaded: None,
        }
    }

    fn reload(&mut self, cwd: &Path) {
        self.path = cwd.to_path_buf();
        self.loaded = Some(Instant::now());
        self.entries = std::fs::read_dir(cwd)
            .map(|rd| {
                rd.flatten()
                    .map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        (name, e.path().is_dir())
                    })
                    .filter(|(name, _)| !name.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default();
        // Directories first, then case-insensitive by name.
        self.entries
            .sort_by_key(|(name, is_dir)| (!*is_dir, name.to_lowercase()));
    }

    /// Draws the browser: a path, then a grid of tiles. Returns text to type into the
    /// terminal when an entry is clicked.
    pub fn show(&mut self, ui: &mut Ui, theme: &Theme, cwd: Option<&Path>) -> Option<String> {
        if let Some(cwd) = cwd {
            let stale = self
                .loaded
                .is_none_or(|t| t.elapsed() > Duration::from_secs(2));
            if cwd != self.path || stale {
                self.reload(cwd);
            }
        }
        let count = self.entries.len();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui::tracked(ui, "FILES", ui::display_bold(12.0), ui::SECONDARY, 3.0);
            // The path, with everything but the current folder played down.
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let shown = match home.as_ref().and_then(|home| self.path.strip_prefix(home).ok()) {
                Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
                Some(rest) => format!("~/{}", rest.display()),
                None => self.path.display().to_string(),
            };
            let (parent, name) = match shown.rsplit_once('/') {
                Some((parent, name)) if !name.is_empty() => (format!("{parent}/"), name.to_string()),
                _ => (String::new(), shown.clone()),
            };
            ui.spacing_mut().item_spacing.x = 0.0;
            ui::text(ui, &truncate_start(&parent, 60), ui::mono(12.0), ui::LABEL);
            ui::text(ui, &name, ui::mono(12.0), ui::TEXT);
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let plural = if count == 1 { "" } else { "S" };
                ui::meta(ui, &format!("{count} ITEM{plural} · FOLLOWS THE TERMINAL"), ui::LABEL);
                let rest = ui.available_rect_before_wrap();
                if rest.width() > 4.0 {
                    ui.painter().hline(rest.x_range(), rest.center().y, Stroke::new(1.0, ui::LINE));
                }
            });
        });
        ui.add_space(8.0);

        let mut typed = None;
        let gap = 8.0;
        let columns = ((ui.available_width() + gap) / (200.0 + gap)).floor().max(1.0);
        let width = ((ui.available_width() + gap) / columns - gap).floor();
        let mut tiles: Vec<(String, bool, Option<String>)> = Vec::new();
        if self.path.parent().is_some() {
            tiles.push(("..".to_string(), true, Some("cd ..\r".to_string())));
        }
        for (name, is_dir) in &self.entries {
            tiles.push((name.clone(), *is_dir, None));
        }
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(gap, gap);
            ui.horizontal_wrapped(|ui| {
                for (name, is_dir, command) in &tiles {
                    let (rect, response) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
                    let painter = ui.painter_at(rect);
                    let line = if response.hovered() { ui::LINE_STRONG } else { ui::LINE };
                    painter.rect_filled(rect, 0.0, ui::GROUND);
                    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, line), StrokeKind::Inside);
                    let icon = pos2(rect.left() + 20.0, rect.center().y);
                    if *is_dir {
                        let color = if name == ".." { ui::SECONDARY } else { theme.main };
                        folder_icon(&painter, icon, color);
                    } else {
                        file_icon(&painter, icon, ui::LABEL);
                    }
                    painter.text(
                        pos2(rect.left() + 38.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        truncate(name, ((width - 48.0) / 7.4) as usize),
                        ui::mono(12.0),
                        if *is_dir && name != ".." { ui::TEXT } else { ui::SECONDARY },
                    );
                    if response.clicked() {
                        typed = command.clone().or_else(|| {
                            let quoted = shell_quote(&self.path.join(name).to_string_lossy());
                            Some(if *is_dir {
                                format!("cd {quoted}\r")
                            } else {
                                format!("{quoted} ")
                            })
                        });
                    }
                }
            });
        });
        typed
    }
}

/// Keeps the end of `s`, which for a path is the part that matters.
fn truncate_start(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let tail: String = s.chars().skip(count - max.saturating_sub(1)).collect();
        format!("…{tail}")
    }
}

fn folder_icon(painter: &eframe::egui::Painter, center: eframe::egui::Pos2, color: eframe::egui::Color32) {
    let (x, y) = (center.x - 8.0, center.y - 6.0);
    let points = [(0.0, 1.0), (5.0, 1.0), (6.5, 3.0), (15.0, 3.0), (15.0, 12.0), (0.0, 12.0), (0.0, 1.0)];
    let points: Vec<_> = points.iter().map(|(dx, dy)| pos2(x + dx, y + dy)).collect();
    painter.line(points, Stroke::new(1.2, color));
}

fn file_icon(painter: &eframe::egui::Painter, center: eframe::egui::Pos2, color: eframe::egui::Color32) {
    let (x, y) = (center.x - 6.0, center.y - 7.0);
    let points = [(0.0, 0.0), (8.0, 0.0), (12.0, 4.0), (12.0, 14.0), (0.0, 14.0), (0.0, 0.0)];
    let points: Vec<_> = points.iter().map(|(dx, dy)| pos2(x + dx, y + dy)).collect();
    painter.line(points, Stroke::new(1.2, color));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn formats_sizes_and_names() {
        assert_eq!(human_bytes(512.0), "512 B");
        assert_eq!(human_bytes(1536.0), "1.5 KB");
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
        assert_eq!(truncate_start("abcdefghij", 5), "…ghij");
        assert_eq!(truncate_start("short", 10), "short");
    }
}
