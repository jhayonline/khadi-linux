//! The user's settings, kept as `key = value` lines in `~/.config/edex-rs/config`.
//!
//! Everything has a default, so the file only holds what was changed. It is written
//! one key at a time, which leaves comments and unknown lines alone.

use crate::geo::{Place, parse_location};
use crate::term::{CursorShape, TermLook};
use crate::theme;
use edex_common::Panels;
use std::path::PathBuf;

pub const MIN_FONT_SIZE: f32 = 10.0;
pub const MAX_FONT_SIZE: f32 = 24.0;
pub const LINE_HEIGHT_RANGE: (f32, f32) = (0.9, 1.6);
pub const SCROLLBACK_CHOICES: [usize; 4] = [1000, 5000, 20000, 100000];
const DEFAULT_FONT_SIZE: f32 = 14.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub theme: String,
    /// Size of the terminal's text.
    pub font_size: f32,
    /// A font file to draw the terminal with, instead of the bundled one.
    pub terminal_font: Option<PathBuf>,
    pub line_height: f32,
    pub cursor: CursorShape,
    pub cursor_blink: bool,
    pub bold_bright: bool,
    pub copy_on_select: bool,
    /// Lines kept above the screen, for new tabs.
    pub scrollback: usize,
    /// Whether the globe turns. Still, it shows this machine's side of the world.
    pub globe_rotation: bool,
    /// Whether addresses are looked up online to place them on the globe.
    pub lookups: bool,
    /// This machine's position, when set by hand.
    pub location: Option<Place>,
    /// Which panels are open when the desktop starts.
    pub panels: Panels,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: theme::DEFAULT_NAME.to_string(),
            font_size: DEFAULT_FONT_SIZE,
            terminal_font: None,
            line_height: 1.0,
            cursor: CursorShape::Block,
            cursor_blink: false,
            bold_bright: true,
            copy_on_select: false,
            scrollback: 5000,
            globe_rotation: true,
            lookups: true,
            location: None,
            panels: Panels::default(),
        }
    }
}

fn path() -> Option<PathBuf> {
    edex_common::input::config_path()
}

impl Config {
    /// Reads settings from the file's text; anything missing or malformed keeps its
    /// default.
    pub fn parse(text: &str) -> Config {
        let mut config = Config::default();
        for line in text.lines() {
            let Some((key, value)) = line.split('#').next().unwrap_or("").split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "theme" if !value.is_empty() => config.theme = value.to_string(),
                "font_size" => {
                    if let Ok(size) = value.parse::<f32>() {
                        config.font_size = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
                    }
                }
                "terminal_font" if !value.is_empty() => config.terminal_font = Some(PathBuf::from(value)),
                "line_height" => {
                    if let Ok(factor) = value.parse::<f32>() {
                        config.line_height = factor.clamp(LINE_HEIGHT_RANGE.0, LINE_HEIGHT_RANGE.1);
                    }
                }
                "cursor" => {
                    config.cursor = match value {
                        "bar" => CursorShape::Bar,
                        "underline" => CursorShape::Underline,
                        _ => CursorShape::Block,
                    }
                }
                "cursor_blink" => config.cursor_blink = value == "on",
                "bold_bright" => config.bold_bright = value != "off",
                "copy_on_select" => config.copy_on_select = value == "on",
                "scrollback" => {
                    if let Ok(lines) = value.parse::<usize>() {
                        config.scrollback = lines.clamp(100, 1_000_000);
                    }
                }
                "globe_rotation" => config.globe_rotation = value != "off",
                "geo" => config.lookups = value != "off",
                "location" => config.location = parse_location(value),
                "panels" => {
                    let open = |name: &str| value.split(',').any(|part| part.trim() == name);
                    config.panels = Panels {
                        left: open("left"),
                        right: open("right"),
                        bottom: open("bottom"),
                    };
                }
                _ => {}
            }
        }
        config
    }

    pub fn load() -> Config {
        path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| Config::parse(&text))
            .unwrap_or_default()
    }

    /// How the terminal should look, from these settings.
    pub fn term_look(&self) -> TermLook {
        TermLook {
            font_size: self.font_size,
            line_height: self.line_height,
            cursor: self.cursor,
            cursor_blink: self.cursor_blink,
            bold_bright: self.bold_bright,
            copy_on_select: self.copy_on_select,
        }
    }

    /// The `panels` value for a set of open panels.
    pub fn panels_value(panels: Panels) -> String {
        let names: Vec<&str> = [("left", panels.left), ("right", panels.right), ("bottom", panels.bottom)]
            .into_iter()
            .filter_map(|(name, open)| open.then_some(name))
            .collect();
        if names.is_empty() { "none".to_string() } else { names.join(", ") }
    }
}

/// Sets one key in the file, or removes it (back to its default) when `value` is
/// `None`, leaving every other line as it was.
pub fn set(key: &str, value: Option<&str>) -> std::io::Result<PathBuf> {
    let path = path().ok_or_else(|| std::io::Error::other("no home directory"))?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = old
        .lines()
        .filter(|line| line.split_once('=').is_none_or(|(k, _)| k.trim() != key))
        .map(str::to_string)
        .collect();
    if let Some(value) = value {
        lines.push(format!("{key} = {value}"));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_keep_their_defaults() {
        assert_eq!(Config::parse(""), Config::default());
        assert_eq!(Config::parse("nonsense\nother = 1\nfont_size = big\nlocation = nowhere\n"), Config::default());
    }

    #[test]
    fn reads_every_setting() {
        let config = Config::parse(
            "# mine\ntheme = matrix\nfont_size = 16\nglobe_rotation = off\ngeo = off # private\n\
             location = 48.85, 2.35, Paris\npanels = left, bottom\n",
        );
        assert_eq!(config.theme, "matrix");
        assert_eq!(config.font_size, 16.0);
        assert!(!config.globe_rotation);
        assert!(!config.lookups);
        assert_eq!(config.location.unwrap().label, "Paris");
        assert_eq!(config.panels, Panels { left: true, right: false, bottom: true });
    }

    #[test]
    fn reads_terminal_settings() {
        let config = Config::parse(
            "terminal_font = /usr/share/fonts/x.ttf\nline_height = 1.3\ncursor = bar\ncursor_blink = on\n\
             bold_bright = off\ncopy_on_select = on\nscrollback = 20000\n",
        );
        assert_eq!(config.terminal_font, Some(PathBuf::from("/usr/share/fonts/x.ttf")));
        assert_eq!(config.line_height, 1.3);
        assert_eq!(config.cursor, CursorShape::Bar);
        assert!(config.cursor_blink && !config.bold_bright && config.copy_on_select);
        assert_eq!(config.scrollback, 20000);
        assert_eq!(Config::parse("line_height = 9\ncursor = star\nscrollback = 1\n").line_height, LINE_HEIGHT_RANGE.1);
        assert_eq!(Config::parse("cursor = star").cursor, CursorShape::Block);
        assert_eq!(Config::parse("scrollback = 1").scrollback, 100);
    }

    #[test]
    fn font_size_stays_readable() {
        assert_eq!(Config::parse("font_size = 2").font_size, MIN_FONT_SIZE);
        assert_eq!(Config::parse("font_size = 200").font_size, MAX_FONT_SIZE);
    }

    #[test]
    fn panels_round_trip() {
        for bits in 0..8 {
            let panels = Panels::from_bits(bits).unwrap();
            let text = format!("panels = {}", Config::panels_value(panels));
            assert_eq!(Config::parse(&text).panels, panels, "{text}");
        }
    }
}
