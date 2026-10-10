//! The two colours the lock screen needs, read from the same theme files the shell uses.
//!
//! This does not share the shell's loader. That one returns `egui::Color32` and pulls
//! in the whole toolkit, which is exactly what a lock screen must not depend on: the
//! fewer things between a password and the screen, the fewer ways it can fail to come
//! up. The format is small enough that reading it twice is cheaper than sharing it.

use serde::Deserialize;

/// The built-in theme, compiled in so that a lock screen still appears on a machine
/// with no theme files at all.
const DEFAULT: &str = include_str!("../../themes/signal.json");

#[derive(Deserialize)]
struct Raw {
    colors: RawColors,
    #[serde(default)]
    terminal: RawTerminal,
}

#[derive(Deserialize)]
struct RawColors {
    r: u8,
    g: u8,
    b: u8,
    black: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawTerminal {
    background: Option<String>,
}

#[derive(Clone, Copy)]
pub struct Theme {
    /// What the typed characters and the line under them are drawn in.
    pub accent: u32,
    /// What covers the screen.
    pub background: u32,
    /// Shown when the password was wrong.
    pub error: u32,
    /// The line, before anything is typed.
    pub muted: u32,
}

/// 0xAARRGGBB, which is what `wl_shm`'s Argb8888 wants on a little-endian machine.
const fn argb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some(0xFF00_0000 | value)
}

/// Mixes towards black, for the line that is not yet lit.
const fn dim(colour: u32, numerator: u32) -> u32 {
    let (r, g, b) = ((colour >> 16) & 0xFF, (colour >> 8) & 0xFF, colour & 0xFF);
    argb(
        (r * numerator / 100) as u8,
        (g * numerator / 100) as u8,
        (b * numerator / 100) as u8,
    )
}

impl Theme {
    /// The theme named in the shell's config, or the built-in one. A theme that will
    /// not parse is ignored rather than fatal: a lock screen in the wrong colours is
    /// a great deal better than no lock screen.
    pub fn load() -> Theme {
        let name = current_theme_name();
        let text = read_theme(&name).unwrap_or_else(|| DEFAULT.to_string());
        let raw: Raw = match serde_json::from_str(&text) {
            Ok(raw) => raw,
            Err(e) => {
                eprintln!("khadi-lock: theme {name} will not parse ({e}); using the built-in one");
                serde_json::from_str(DEFAULT).expect("the built-in theme must parse")
            }
        };
        let accent = argb(raw.colors.r, raw.colors.g, raw.colors.b);
        let background = raw
            .terminal
            .background
            .as_deref()
            .and_then(parse_hex)
            .or_else(|| raw.colors.black.as_deref().and_then(parse_hex))
            .unwrap_or(argb(0x06, 0x08, 0x0B));
        Theme {
            accent,
            background,
            error: argb(0xFF, 0x7A, 0x6B),
            muted: dim(accent, 28),
        }
    }
}

/// `theme = <name>` in the shell's config file.
fn current_theme_name() -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return "signal".into();
    };
    let path = std::path::Path::new(&home).join(".config/khadi/config");
    let Ok(text) = std::fs::read_to_string(path) else {
        return "signal".into();
    };
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == "theme"
        {
            return value.trim().to_string();
        }
    }
    "signal".into()
}

fn read_theme(name: &str) -> Option<String> {
    let mut places: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        places.push(std::path::Path::new(&home).join(".config/khadi/themes"));
    }
    places.push("/usr/local/share/khadi/themes".into());
    places.push("/usr/share/khadi/themes".into());
    places
        .into_iter()
        .find_map(|dir| std::fs::read_to_string(dir.join(format!("{name}.json"))).ok())
}
