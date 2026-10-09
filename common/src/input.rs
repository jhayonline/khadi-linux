//! Settings for keyboards, mice and touchpads. The shell's settings screen writes them
//! to the configuration file; the compositor, which owns the devices, reads them there.

use std::path::PathBuf;

/// Where the settings live: `key = value` lines, shared by the shell and the compositor.
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("edex-rs/config"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputSettings {
    /// Content follows the fingers, as on a phone.
    pub touchpad_natural_scroll: bool,
    pub touchpad_tap: bool,
    /// Ignore the touchpad while keys are being pressed.
    pub touchpad_while_typing_off: bool,
    /// Pointer speed, from -1 (slowest) to 1 (fastest).
    pub touchpad_speed: f32,
    pub mouse_natural_scroll: bool,
    pub mouse_speed: f32,
    pub mouse_left_handed: bool,
    /// An XKB layout such as `us` or `de`; empty for the system's default.
    pub keyboard_layout: String,
    pub keyboard_variant: String,
    /// Milliseconds a key is held before it starts repeating.
    pub repeat_delay: u32,
    /// Repeats per second.
    pub repeat_rate: u32,
}

impl Default for InputSettings {
    fn default() -> Self {
        InputSettings {
            touchpad_natural_scroll: true,
            touchpad_tap: true,
            touchpad_while_typing_off: true,
            touchpad_speed: 0.0,
            mouse_natural_scroll: false,
            mouse_speed: 0.0,
            mouse_left_handed: false,
            keyboard_layout: String::new(),
            keyboard_variant: String::new(),
            repeat_delay: 250,
            repeat_rate: 30,
        }
    }
}

pub const REPEAT_DELAY_RANGE: (u32, u32) = (150, 800);
pub const REPEAT_RATE_RANGE: (u32, u32) = (10, 60);

impl InputSettings {
    /// Reads the settings from the configuration file's text. Anything missing or
    /// malformed keeps its default.
    pub fn parse(text: &str) -> InputSettings {
        let mut settings = InputSettings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split('#').next().unwrap_or("").split_once('=') else {
                continue;
            };
            let value = value.trim();
            let switch = |default: bool| match value {
                "on" => true,
                "off" => false,
                _ => default,
            };
            let speed = |default: f32| value.parse::<f32>().map_or(default, |speed| speed.clamp(-1.0, 1.0));
            // Layout names are passed to the keymap compiler: letters, digits and a few
            // marks only.
            let name = |default: &str| {
                let plain = value.chars().all(|c| c.is_ascii_alphanumeric() || "_-,".contains(c));
                if plain { value.to_string() } else { default.to_string() }
            };
            match key.trim() {
                "touchpad_natural_scroll" => settings.touchpad_natural_scroll = switch(settings.touchpad_natural_scroll),
                "touchpad_tap" => settings.touchpad_tap = switch(settings.touchpad_tap),
                "touchpad_while_typing_off" => {
                    settings.touchpad_while_typing_off = switch(settings.touchpad_while_typing_off)
                }
                "touchpad_speed" => settings.touchpad_speed = speed(settings.touchpad_speed),
                "mouse_natural_scroll" => settings.mouse_natural_scroll = switch(settings.mouse_natural_scroll),
                "mouse_speed" => settings.mouse_speed = speed(settings.mouse_speed),
                "mouse_left_handed" => settings.mouse_left_handed = switch(settings.mouse_left_handed),
                "keyboard_layout" => settings.keyboard_layout = name(&settings.keyboard_layout),
                "keyboard_variant" => settings.keyboard_variant = name(&settings.keyboard_variant),
                "keyboard_repeat_delay" => {
                    if let Ok(delay) = value.parse::<u32>() {
                        settings.repeat_delay = delay.clamp(REPEAT_DELAY_RANGE.0, REPEAT_DELAY_RANGE.1);
                    }
                }
                "keyboard_repeat_rate" => {
                    if let Ok(rate) = value.parse::<u32>() {
                        settings.repeat_rate = rate.clamp(REPEAT_RATE_RANGE.0, REPEAT_RATE_RANGE.1);
                    }
                }
                _ => {}
            }
        }
        settings
    }

    /// Reads the settings from the configuration file, or the defaults without one.
    pub fn load() -> InputSettings {
        config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| InputSettings::parse(&text))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_what_laptops_usually_do() {
        let settings = InputSettings::parse("");
        assert!(settings.touchpad_natural_scroll);
        assert!(settings.touchpad_tap);
        assert!(!settings.mouse_natural_scroll);
        assert_eq!(settings, InputSettings::default());
    }

    #[test]
    fn reads_every_setting() {
        let settings = InputSettings::parse(
            "touchpad_natural_scroll = off\ntouchpad_tap = off\ntouchpad_while_typing_off = off\n\
             touchpad_speed = 0.4\nmouse_natural_scroll = on\nmouse_speed = -0.5\nmouse_left_handed = on\n\
             keyboard_layout = de\nkeyboard_variant = nodeadkeys\nkeyboard_repeat_delay = 400\n\
             keyboard_repeat_rate = 40 # fast\ntheme = matrix\n",
        );
        assert!(!settings.touchpad_natural_scroll && !settings.touchpad_tap && !settings.touchpad_while_typing_off);
        assert_eq!(settings.touchpad_speed, 0.4);
        assert!(settings.mouse_natural_scroll && settings.mouse_left_handed);
        assert_eq!(settings.mouse_speed, -0.5);
        assert_eq!((settings.keyboard_layout.as_str(), settings.keyboard_variant.as_str()), ("de", "nodeadkeys"));
        assert_eq!((settings.repeat_delay, settings.repeat_rate), (400, 40));
    }

    #[test]
    fn bad_values_are_ignored_or_clamped() {
        let settings = InputSettings::parse(
            "touchpad_speed = 9\nmouse_speed = fast\nkeyboard_repeat_delay = 1\nkeyboard_repeat_rate = 999\n\
             touchpad_tap = maybe\nkeyboard_layout = us; rm -rf /\n",
        );
        assert_eq!(settings.touchpad_speed, 1.0);
        assert_eq!(settings.mouse_speed, 0.0);
        assert_eq!(settings.repeat_delay, REPEAT_DELAY_RANGE.0);
        assert_eq!(settings.repeat_rate, REPEAT_RATE_RANGE.1);
        assert!(settings.touchpad_tap);
        assert_eq!(settings.keyboard_layout, "");
    }
}
