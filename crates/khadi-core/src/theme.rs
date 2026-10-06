//! Theme loading.
//!
//! khadi-hud does NOT re-derive the alpha ramp. `khadi-theme` resolves it and
//! writes `$XDG_CONFIG_HOME/khadi/theme.toml`; this reads that. Two
//! implementations of the same derivation is two chances to disagree, and the
//! whole point of Phase 2 was one source of truth.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, env, path::PathBuf};

/// 24-bit colour. Terminals get this directly; the ramp is already flattened
/// against the background by khadi-theme, because no terminal does alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    fn parse(s: &str) -> Result<Self> {
        let h = s.trim_start_matches('#');
        anyhow::ensure!(h.len() == 6, "not a #rrggbb colour: {s:?}");
        Ok(Self {
            r: u8::from_str_radix(&h[0..2], 16)?,
            g: u8::from_str_radix(&h[2..4], 16)?,
            b: u8::from_str_radix(&h[4..6], 16)?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct Raw {
    meta: BTreeMap<String, String>,
    role: BTreeMap<String, String>,
    ramp: BTreeMap<String, String>,
    #[serde(default)]
    font: BTreeMap<String, String>,
}

/// The semantic roles. Widgets reference these, never raw ramp indices, so
/// retuning the look stays a change in the theme file and nowhere else.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub text: Rgb,
    pub text_dim: Rgb,
    pub text_muted: Rgb,
    pub rule: Rgb,
    pub rule_faint: Rgb,
    pub ground: Rgb,
    pub ramp: Vec<Rgb>, // a10..a90, index 0 = a10
    pub mono: String,
}

impl Theme {
    pub fn path() -> PathBuf {
        let base = env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(env::var("HOME").unwrap_or_default()).join(".config")
            });
        base.join("khadi").join("theme.toml")
    }

    pub fn load() -> Result<Self> {
        let p = Self::path();
        let text = std::fs::read_to_string(&p)
            .with_context(|| format!("no theme at {}; run: khadi-theme build <name>", p.display()))?;
        Self::from_str(&text)
    }

    pub fn from_str(text: &str) -> Result<Self> {
        let raw: Raw = toml::from_str(text).context("theme.toml is not valid")?;
        let role = |k: &str| -> Result<Rgb> {
            Rgb::parse(raw.role.get(k).with_context(|| format!("theme has no role.{k}"))?)
        };
        let mut ramp = Vec::with_capacity(9);
        for i in 1..=9 {
            let k = format!("a{}0", i);
            ramp.push(Rgb::parse(
                raw.ramp.get(&k).with_context(|| format!("theme has no ramp.{k}"))?,
            )?);
        }
        Ok(Self {
            name: raw.meta.get("name").cloned().unwrap_or_else(|| "unknown".into()),
            text: role("text")?,
            text_dim: role("text_dim")?,
            text_muted: role("text_muted")?,
            rule: role("rule")?,
            rule_faint: role("rule_faint")?,
            ground: role("ground")?,
            ramp,
            mono: raw.font.get("mono").cloned().unwrap_or_else(|| "monospace".into()),
        })
    }

    /// Ramp step by alpha percent, clamped. `a(30)` is the rule colour — the
    /// 0.3 that carries the whole design.
    pub fn a(&self, pct: u8) -> Rgb {
        let i = ((pct.clamp(10, 90) / 10) as usize).saturating_sub(1);
        self.ramp[i.min(self.ramp.len() - 1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
[meta]
name = "tron"
[role]
text = "#aacfd1"
text_dim = "#89a7aa"
text_muted = "#586c6f"
rule = "#364448"
rule_faint = "#263034"
ground = "#05080d"
[ramp]
a10 = "#161c21"
a20 = "#263034"
a30 = "#364448"
a40 = "#47585b"
a50 = "#586c6f"
a60 = "#687f83"
a70 = "#789396"
a80 = "#89a7aa"
a90 = "#9abbbd"
[font]
mono = "Iosevka Term"
"##;

    #[test]
    fn parses_the_real_shape() {
        let t = Theme::from_str(SAMPLE).unwrap();
        assert_eq!(t.name, "tron");
        assert_eq!(t.text, Rgb { r: 170, g: 207, b: 209 });
        assert_eq!(t.rule, Rgb { r: 0x36, g: 0x44, b: 0x48 });
        assert_eq!(t.mono, "Iosevka Term");
    }

    #[test]
    fn a30_is_the_rule_colour() {
        let t = Theme::from_str(SAMPLE).unwrap();
        assert_eq!(t.a(30), t.rule, "ramp a30 and role.rule must agree");
    }

    #[test]
    fn missing_role_is_an_error_not_a_default() {
        // Silently defaulting a missing colour produces a theme that looks
        // almost right, which is worse than one that refuses to load.
        let bad = SAMPLE.replace(r##"rule = "#364448""##, "");
        assert!(Theme::from_str(&bad).is_err());
    }
}
