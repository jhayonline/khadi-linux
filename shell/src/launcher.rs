//! The application launcher: a search box over the installed applications, shown in
//! place of the terminal.

use crate::apps::{self, AppEntry};
use crate::theme::Theme;
use crate::ui;
use eframe::egui::{
    Align, Align2, Key, Layout, Modifiers, Rect, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui,
    UiBuilder, vec2,
};

const ROW_HEIGHT: f32 = 56.0;

#[derive(Default)]
pub struct Launcher {
    pub open: bool,
    query: String,
    selected: usize,
    entries: Vec<AppEntry>,
    /// The last thing that went wrong, shown under the search box.
    error: Option<String>,
}

impl Launcher {
    pub fn open(&mut self) {
        // Read afresh each time: applications get installed and removed.
        self.entries = apps::scan();
        self.query.clear();
        self.selected = 0;
        self.error = None;
        self.open = true;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.open = false;
        } else {
            self.open();
        }
    }

    /// The entries matching the query: names starting with it first, then names
    /// containing it.
    fn matches(&self) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();
        let mut starts = Vec::new();
        let mut contains = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            let name = entry.name.to_lowercase();
            if name.starts_with(&query) {
                starts.push(index);
            } else if name.contains(&query) {
                contains.push(index);
            }
        }
        starts.extend(contains);
        starts
    }

    /// Draws the launcher into `rect`. Returns an application that must be run inside
    /// a terminal, when the user picked one; others are started directly.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme) -> Option<AppEntry> {
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.open = false;
            return None;
        }

        // Wide margins on a wide workspace, so the list does not stretch edge to edge.
        let margin = (rect.width() * 0.09).clamp(16.0, 120.0);
        let area = Rect::from_min_max(rect.min + vec2(margin, 32.0), rect.max - vec2(margin, 0.0));
        let before = self.query.clone();
        let mut picked = None;
        ui.allocate_new_ui(UiBuilder::new().max_rect(area), |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui::tracked(ui, "OPEN AN APPLICATION", ui::display_bold(12.0), ui::SECONDARY, 3.0);

            // The search field: a box in the accent, a lens, the query, the count.
            let (field, _) = ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::hover());
            ui.painter().rect_filled(field, 0.0, ui::PANEL);
            ui.painter()
                .rect_stroke(field, 0.0, Stroke::new(1.0, theme.main), StrokeKind::Inside);
            let lens = field.left_center() + vec2(26.0, -1.0);
            ui.painter().circle_stroke(lens, 5.5, Stroke::new(1.5, theme.main));
            ui.painter()
                .line_segment([lens + vec2(4.0, 4.0), lens + vec2(9.0, 9.0)], Stroke::new(1.5, theme.main));
            let matches = {
                let input = Rect::from_min_max(field.min + vec2(50.0, 14.0), field.max - vec2(110.0, 14.0));
                let search = ui.put(
                    input,
                    TextEdit::singleline(&mut self.query)
                        .frame(false)
                        .font(ui::mono(22.0))
                        .text_color(ui::TEXT)
                        .hint_text("type a name"),
                );
                search.request_focus();
                self.matches()
            };
            ui.painter().text(
                field.right_center() - vec2(18.0, 0.0),
                Align2::RIGHT_CENTER,
                format!("{} OF {}", matches.len(), self.entries.len()),
                ui::mono(12.0),
                ui::LABEL,
            );
            if let Some(error) = &self.error {
                ui::text(ui, error, ui::mono(12.0), ui::CAUTION);
            }
            ui.add_space(12.0);

            if self.query != before {
                self.selected = 0;
            }
            if down {
                self.selected += 1;
            }
            if up {
                self.selected = self.selected.saturating_sub(1);
            }
            self.selected = self.selected.min(matches.len().saturating_sub(1));
            if enter {
                picked = matches.get(self.selected).copied();
            }

            // The list on the left; on a wide workspace, the selection's details beside it.
            let detail_width = if ui.available_width() > 760.0 { 320.0 } else { 0.0 };
            let list_width = ui.available_width() - detail_width - if detail_width > 0.0 { 24.0 } else { 0.0 };
            let selected_entry = matches.get(self.selected).map(|index| self.entries[*index].clone());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 24.0;
                ui.allocate_ui_with_layout(vec2(list_width, ui.available_height()), Layout::top_down(Align::Min), |ui| {
                    if matches.is_empty() {
                        ui::text(ui, "No application by that name", ui::mono(13.0), ui::SECONDARY);
                    }
                    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for (row, index) in matches.iter().enumerate() {
                            let entry = &self.entries[*index];
                            let (row_rect, response) =
                                ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
                            let selected = row == self.selected;
                            let painter = ui.painter_at(row_rect);
                            if selected {
                                painter.rect_filled(row_rect, 0.0, theme.main);
                                if up || down {
                                    ui.scroll_to_rect(row_rect, None);
                                }
                            } else {
                                if response.hovered() {
                                    painter.rect_filled(row_rect, 0.0, ui::PANEL);
                                }
                                painter.hline(row_rect.x_range(), row_rect.bottom() - 0.5, Stroke::new(1.0, ui::TRACK));
                            }
                            let (ink, quiet, edge) = if selected {
                                (ui::GROUND, theme.muted_on_accent(), ui::GROUND)
                            } else {
                                (ui::TEXT, ui::LABEL, ui::LINE_STRONG)
                            };
                            // A square with the name's initial stands in for an icon.
                            let badge = Rect::from_center_size(row_rect.left_center() + vec2(32.0, 0.0), vec2(28.0, 28.0));
                            painter.rect_stroke(badge, 0.0, Stroke::new(1.5, edge), StrokeKind::Inside);
                            let initial: String = entry.name.chars().take(1).flat_map(char::to_uppercase).collect();
                            painter.text(badge.center(), Align2::CENTER_CENTER, initial, ui::display_bold(14.0), if selected { ink } else { ui::SECONDARY });
                            let font = if selected { ui::display_bold(18.0) } else { ui::display(18.0) };
                            painter.text(row_rect.left_center() + vec2(62.0, 0.0), Align2::LEFT_CENTER, &entry.name, font, ink);
                            let note = match (selected, entry.terminal) {
                                (true, _) => "ENTER",
                                (false, true) => "RUNS IN A TERMINAL TAB",
                                (false, false) => "",
                            };
                            painter.text(row_rect.right_center() - vec2(18.0, 0.0), Align2::RIGHT_CENTER, note, ui::mono(11.0), quiet);
                            if response.clicked() {
                                picked = Some(*index);
                            }
                        }
                    });
                });
                if let (true, Some(entry)) = (detail_width > 0.0, &selected_entry) {
                    ui.allocate_ui_with_layout(vec2(detail_width, 0.0), Layout::top_down(Align::Min), |ui| {
                        eframe::egui::Frame::new()
                            .fill(ui::PANEL)
                            .stroke(Stroke::new(1.0, ui::LINE))
                            .inner_margin(eframe::egui::Margin::same(18))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 12.0;
                                ui::tracked(ui, "SELECTED", ui::display_bold(12.0), ui::SECONDARY, 3.0);
                                ui::text(ui, &crate::panels::truncate(&entry.name, 24), ui::display_bold(22.0), ui::TEXT);
                                ui::text(ui, &crate::panels::truncate(&entry.exec.join(" "), 38), ui::mono(12.0), ui::SECONDARY);
                                let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                                ui.painter().rect_filled(line, 0.0, ui::LINE);
                                ui::key_row(ui, "Open", "ENTER");
                                ui::key_row(ui, "Move selection", "↑ ↓");
                                ui::key_row(ui, "Back to terminal", "ESC");
                            });
                    });
                }
            });
        });

        let entry = self.entries.get(picked?)?.clone();
        if entry.terminal {
            self.open = false;
            return Some(entry);
        }
        match apps::launch(&entry) {
            Ok(()) => self.open = false,
            Err(e) => self.error = Some(format!("Cannot start {}: {e}", entry.name)),
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matches_come_first() {
        let entry = |name: &str| AppEntry {
            name: name.into(),
            exec: vec!["x".into()],
            terminal: false,
        };
        let mut launcher = Launcher {
            entries: vec![entry("Brave"), entry("Firefox"), entry("Fire Alarm"), entry("Campfire")],
            ..Default::default()
        };
        assert_eq!(launcher.matches(), [0, 1, 2, 3]);
        launcher.query = " FIRE".into();
        assert_eq!(launcher.matches(), [1, 2, 3]);
        launcher.query = "zzz".into();
        assert!(launcher.matches().is_empty());
    }
}
