//! Display settings: reports the monitors to the shell and carries out the layouts
//! chosen there.
//!
//! A new layout is tried first and saved only once the user confirms it. Without that
//! confirmation it is undone, because a mode the monitor cannot show leaves nothing on
//! screen to click.

use std::time::{Duration, Instant};

use crate::{
    EdexComp,
    monitors::{self, Saved},
};
use edex_common::ipc::{DisplayChoice, DisplayInfo, Displays, Mode};

/// How long a new layout waits to be confirmed.
pub const CONFIRM_WITHIN: Duration = Duration::from_secs(15);

/// Turns the user's choices into a layout: monitors side by side in the order given,
/// tops aligned. Returns `None` unless they name exactly the connected monitors, each
/// with a mode it offers.
pub fn layout_for(choices: &[DisplayChoice], connected: &[DisplayInfo]) -> Option<Vec<Saved>> {
    if choices.len() != connected.len() {
        return None;
    }
    // Exactly one main display: the first one marked, or the leftmost.
    let main = choices.iter().position(|choice| choice.main).unwrap_or(0);
    let mut x = 0;
    let mut layout = Vec::new();
    for (index, choice) in choices.iter().enumerate() {
        let monitor = connected.iter().find(|monitor| monitor.name == choice.name)?;
        if !monitor.modes.contains(&choice.mode) || layout.iter().any(|saved: &Saved| saved.connector == monitors::gnome_name(&choice.name)) {
            return None;
        }
        layout.push(Saved {
            connector: monitors::gnome_name(&choice.name),
            width: choice.mode.width as i32,
            height: choice.mode.height as i32,
            rate: choice.mode.refresh as f64 / 1000.0,
            x,
            y: 0,
            scale: 1.0,
            primary: index == main,
        });
        x += choice.mode.width as i32;
    }
    Some(layout)
}

impl EdexComp {
    /// What the shell is told about the monitors.
    pub fn display_report(&self) -> Displays {
        let monitors = self.hardware_displays().unwrap_or_else(|| {
            // In a window there is one display, and its size is the window's.
            self.screens
                .iter()
                .enumerate()
                .map(|(index, rect)| {
                    let mode = Mode {
                        width: rect.size.w.max(1) as u32,
                        height: rect.size.h.max(1) as u32,
                        refresh: 60_000,
                    };
                    DisplayInfo {
                        name: format!("WINDOW-{}", index + 1),
                        main: index == 0,
                        x: rect.loc.x,
                        y: rect.loc.y,
                        current: mode,
                        modes: vec![mode],
                    }
                })
                .collect()
        });
        let pending = self.display_pending.as_ref().map_or(0, |(_, since)| {
            CONFIRM_WITHIN.saturating_sub(since.elapsed()).as_secs() as u32 + 1
        });
        Displays { pending, monitors }
    }

    /// Tries a layout chosen in the settings. It stays only if confirmed in time.
    pub fn apply_displays(&mut self, choices: Vec<DisplayChoice>) {
        let Some(connected) = self.hardware_displays() else {
            tracing::info!("display settings are ignored when running in a window");
            return;
        };
        let Some(layout) = layout_for(&choices, &connected) else {
            tracing::warn!(?choices, "ignoring a display layout that does not fit the monitors");
            return;
        };
        tracing::info!(?layout, "trying a display layout");
        // A second change before the first is confirmed still goes back to where
        // things stood before either.
        let previous = match self.display_pending.take() {
            Some((previous, _)) => previous,
            None => self.display_override.take(),
        };
        self.display_pending = Some((previous, Instant::now()));
        self.display_override = Some(layout);
        self.reapply_displays();
    }

    /// The user can see the new layout and wants it: save it.
    pub fn keep_displays(&mut self) {
        if self.display_pending.take().is_none() {
            return;
        }
        if let Some(layout) = &self.display_override {
            match monitors::save_own(layout) {
                Ok(()) => tracing::info!("display layout saved"),
                Err(e) => tracing::error!("cannot save the display layout: {e}"),
            }
        }
    }

    /// Goes back to the layout in force before the one being tried.
    pub fn revert_displays(&mut self) {
        let Some((previous, _)) = self.display_pending.take() else {
            return;
        };
        tracing::info!("display layout not confirmed, going back");
        match previous {
            Some(layout) => {
                self.display_override = Some(layout);
                self.reapply_displays();
            }
            // With nothing held in memory the saved layouts decide again. An empty
            // layout matches no monitor, so each is set up afresh from what is saved.
            None => {
                self.display_override = Some(Vec::new());
                self.reapply_displays();
                self.display_override = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(width: u32, height: u32, refresh: u32) -> Mode {
        Mode { width, height, refresh }
    }

    fn connected() -> Vec<DisplayInfo> {
        vec![
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
                modes: vec![mode(3840, 2160, 30000), mode(1920, 1080, 60000)],
            },
        ]
    }

    fn choice(name: &str, mode: Mode, main: bool) -> DisplayChoice {
        DisplayChoice { name: name.into(), mode, main }
    }

    #[test]
    fn lays_monitors_out_left_to_right() {
        let layout = layout_for(
            &[choice("HDMI-A-1", mode(3840, 2160, 30000), true), choice("eDP-1", mode(1280, 720, 60000), false)],
            &connected(),
        )
        .unwrap();
        assert_eq!(layout[0].connector, "HDMI-1", "saved under the name GNOME uses");
        assert_eq!((layout[0].x, layout[0].width, layout[0].primary), (0, 3840, true));
        assert_eq!((layout[1].x, layout[1].width, layout[1].primary), (3840, 1280, false));
        assert_eq!(layout[0].rate, 30.0);
    }

    #[test]
    fn there_is_always_exactly_one_main_display() {
        let modes = (mode(1920, 1080, 60052), mode(1920, 1080, 60000));
        let none = layout_for(&[choice("eDP-1", modes.0, false), choice("HDMI-A-1", modes.1, false)], &connected()).unwrap();
        assert_eq!(none.iter().filter(|saved| saved.primary).count(), 1);
        assert!(none[0].primary);
        let both = layout_for(&[choice("eDP-1", modes.0, true), choice("HDMI-A-1", modes.1, true)], &connected()).unwrap();
        assert_eq!(both.iter().filter(|saved| saved.primary).count(), 1);
    }

    #[test]
    fn rejects_layouts_that_do_not_fit() {
        let good = mode(1920, 1080, 60052);
        let hdmi = mode(1920, 1080, 60000);
        assert_eq!(layout_for(&[choice("eDP-1", good, true)], &connected()), None, "a monitor is missing");
        assert_eq!(layout_for(&[choice("eDP-1", good, true), choice("DP-9", hdmi, false)], &connected()), None, "not connected");
        assert_eq!(layout_for(&[choice("eDP-1", mode(800, 600, 60000), true), choice("HDMI-A-1", hdmi, false)], &connected()), None, "not a mode it offers");
        assert_eq!(layout_for(&[choice("eDP-1", good, true), choice("eDP-1", good, false)], &connected()), None, "named twice");
    }
}
