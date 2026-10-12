//! Loads eDEX-UI theme files (the original JSON format) into egui colors.

use eframe::egui::Color32;
use serde::Deserialize;
use std::path::PathBuf;

/// The built-in theme, and its name.
pub const DEFAULT_NAME: &str = "signal";
const DEFAULT_THEME: &str = include_str!("../../themes/signal.json");

#[derive(Deserialize)]
struct RawTheme {
    colors: RawColors,
    #[serde(default)]
    terminal: RawTerminal,
}

#[derive(Deserialize)]
struct RawColors {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawTerminal {
    foreground: Option<String>,
    cursor: Option<String>,
    black: Option<String>,
    red: Option<String>,
    green: Option<String>,
    yellow: Option<String>,
    blue: Option<String>,
    magenta: Option<String>,
    cyan: Option<String>,
    white: Option<String>,
    bright_black: Option<String>,
    bright_red: Option<String>,
    bright_green: Option<String>,
    bright_yellow: Option<String>,
    bright_blue: Option<String>,
    bright_magenta: Option<String>,
    bright_cyan: Option<String>,
    bright_white: Option<String>,
}

/// What a theme decides: the accent, and the terminal's colours. The interface's greys
/// are fixed (see `ui`), so the look holds together under any theme.
#[derive(Clone)]
pub struct Theme {
    /// The accent: what is live, selected, or has the keyboard.
    pub main: Color32,
    pub term_fg: Color32,
    pub cursor: Color32,
    pub ansi: [Color32; 16],
}

const XTERM_16: [(u8, u8, u8); 16] = [
    (0x2e, 0x34, 0x36),
    (0xcc, 0x00, 0x00),
    (0x4e, 0x9a, 0x06),
    (0xc4, 0xa0, 0x00),
    (0x34, 0x65, 0xa4),
    (0x75, 0x50, 0x7b),
    (0x06, 0x98, 0x9a),
    (0xd3, 0xd7, 0xcf),
    (0x55, 0x57, 0x53),
    (0xef, 0x29, 0x29),
    (0x8a, 0xe2, 0x34),
    (0xfc, 0xe9, 0x4f),
    (0x72, 0x9f, 0xcf),
    (0xad, 0x7f, 0xa8),
    (0x34, 0xe2, 0xe2),
    (0xee, 0xee, 0xec),
];

/// See [`Theme::list`].
pub fn installed() -> Vec<(String, Color32)> {
    Theme::list()
}

/// Parses `#rgb`, `#rrggbb`, `rgb(r,g,b)` and `rgba(r,g,b,a)`.
fn parse_color(s: &str) -> Option<Color32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let hex = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => hex.to_string(),
            _ => return None,
        };
        let v = u32::from_str_radix(&hex, 16).ok()?;
        return Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
    }
    let inner = s
        .strip_prefix("rgba(")
        .or_else(|| s.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<f32> = inner
        .split(',')
        .map(|p| p.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    match parts.as_slice() {
        [r, g, b] => Some(Color32::from_rgb(*r as u8, *g as u8, *b as u8)),
        [r, g, b, a] => Some(Color32::from_rgba_unmultiplied(
            *r as u8,
            *g as u8,
            *b as u8,
            (a.clamp(0.0, 1.0) * 255.0) as u8,
        )),
        _ => None,
    }
}

impl Theme {
    pub fn parse(json: &str) -> anyhow::Result<Theme> {
        let raw: RawTheme = serde_json::from_str(json)?;
        let main = Color32::from_rgb(raw.colors.r, raw.colors.g, raw.colors.b);
        let col = |s: &Option<String>, fallback: Color32| {
            s.as_deref().and_then(parse_color).unwrap_or(fallback)
        };
        let t = &raw.terminal;
        let overrides = [
            &t.black,
            &t.red,
            &t.green,
            &t.yellow,
            &t.blue,
            &t.magenta,
            &t.cyan,
            &t.white,
            &t.bright_black,
            &t.bright_red,
            &t.bright_green,
            &t.bright_yellow,
            &t.bright_blue,
            &t.bright_magenta,
            &t.bright_cyan,
            &t.bright_white,
        ];
        let mut ansi = [Color32::BLACK; 16];
        for (i, slot) in ansi.iter_mut().enumerate() {
            let (r, g, b) = XTERM_16[i];
            *slot = col(overrides[i], Color32::from_rgb(r, g, b));
        }
        Ok(Theme {
            main,
            term_fg: col(&t.foreground, crate::ui::TEXT),
            cursor: col(&t.cursor, main),
            ansi,
        })
    }

    /// Directories searched for `<name>.json`, in priority order.
    pub fn search_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
        if let Some(config) = config {
            dirs.push(config.join("khadi/themes"));
        }
        dirs.push(PathBuf::from("/usr/local/share/khadi/themes"));
        dirs.push(PathBuf::from("themes"));
        dirs
    }

    /// Loads a theme by name or by path to a JSON file, falling back to the built-in one.
    pub fn load(name: &str) -> Theme {
        let mut candidates = vec![PathBuf::from(name)];
        candidates.extend(
            Self::search_dirs()
                .into_iter()
                .map(|d| d.join(format!("{name}.json"))),
        );
        for path in candidates {
            let Ok(json) = std::fs::read_to_string(&path) else {
                continue;
            };
            match Theme::parse(&json) {
                Ok(theme) => return theme,
                Err(e) => eprintln!("khadi: cannot parse theme {}: {e}", path.display()),
            }
        }
        if name != DEFAULT_NAME {
            eprintln!("khadi: theme '{name}' not found, using {DEFAULT_NAME}");
        }
        Theme::parse(DEFAULT_THEME).expect("built-in theme is valid")
    }

    /// The themes that can be chosen: the built-in one, then every file found, by name,
    /// each with its accent.
    pub fn list() -> Vec<(String, Color32)> {
        let builtin = Theme::parse(DEFAULT_THEME).expect("built-in theme is valid");
        let mut themes = vec![(DEFAULT_NAME.to_string(), builtin.main)];
        let mut found: Vec<(String, Color32)> = Vec::new();
        for dir in Self::search_dirs() {
            let Ok(listing) = std::fs::read_dir(dir) else {
                continue;
            };
            for path in listing.flatten().map(|entry| entry.path()) {
                let Some(name) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
                    continue;
                };
                let known = name == DEFAULT_NAME || found.iter().any(|(other, _)| *other == name);
                if path.extension().is_some_and(|ext| ext == "json") && !known {
                    if let Some(theme) = std::fs::read_to_string(&path).ok().and_then(|json| Theme::parse(&json).ok()) {
                        found.push((name, theme.main));
                    }
                }
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        themes.extend(found);
        themes
    }

    /// The accent mixed towards the ground by `amount` (0 is the accent itself).
    fn toward_ground(&self, amount: f32) -> Color32 {
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
        let ground = crate::ui::GROUND;
        Color32::from_rgb(
            mix(self.main.r(), ground.r()),
            mix(self.main.g(), ground.g()),
            mix(self.main.b(), ground.b()),
        )
    }

    /// The accent's quieter partner: second segments of bars, the globe's land.
    pub fn muted(&self) -> Color32 {
        self.toward_ground(0.55)
    }

    /// A faint wash of the accent, behind what is active.
    pub fn tint(&self) -> Color32 {
        self.toward_ground(0.9)
    }

    /// Secondary text on top of an accent fill.
    pub fn muted_on_accent(&self) -> Color32 {
        self.toward_ground(0.75)
    }

    /// The colour behind the terminal's text selection.
    pub fn selection(&self) -> Color32 {
        self.toward_ground(0.7)
    }

    /// Resolves an xterm 256-color index.
    pub fn indexed(&self, idx: u8) -> Color32 {
        match idx {
            0..=15 => self.ansi[idx as usize],
            16..=231 => {
                let i = idx - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
            }
            _ => {
                let v = 8 + (idx - 232) * 10;
                Color32::from_rgb(v, v, v)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_color_formats() {
        assert_eq!(parse_color("#aacfd1"), Some(Color32::from_rgb(170, 207, 209)));
        assert_eq!(parse_color("#fff"), Some(Color32::WHITE));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Color32::from_rgb(1, 2, 3)));
        assert!(parse_color("rgba(170,207,209,0.3)").is_some());
        assert_eq!(parse_color("nonsense"), None);
    }

    #[test]
    fn every_bundled_theme_parses() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../themes");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let json = std::fs::read_to_string(&path).unwrap();
            Theme::parse(&json).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            count += 1;
        }
        assert!(count > 0);
    }
}
