use smithay::{
    backend::session::Session,
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent, KeyState,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::SERIAL_COUNTER,
};

use crate::state::EdexComp;

/// A compositor shortcut, as opposed to a key press that belongs to the focused client.
enum Action {
    Cycle,
    Close,
    /// Move the current application to the next display.
    MoveDisplay,
    Launcher,
    Settings,
    Split,
    Fullscreen,
    /// Give the keyboard to the left (false) or right (true) half of a split.
    FocusHalf(bool),
    Panel(khadi_common::Panel),
    RestartShell,
    /// A volume or brightness key.
    Media(Media),
    /// Switch to another virtual terminal (Ctrl+Alt+F1..F12).
    SwitchVt(i32),
    /// End the session (Ctrl+Alt+Backspace).
    Quit,
}

#[derive(Clone, Copy)]
enum Media {
    VolumeUp,
    VolumeDown,
    Mute,
    BrightnessUp,
    BrightnessDown,
}

/// Runs a helper program without waiting for it.
fn run(program: &str, args: &[&str]) {
    match std::process::Command::new(program).args(args).spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => tracing::warn!("cannot run {program}: {e}"),
    }
}

/// Steps the backlight by a tenth of its range, through the login service, which lets
/// the active session do that without special permissions.
fn step_brightness(up: bool) {
    let Some(device) = std::fs::read_dir("/sys/class/backlight")
        .ok()
        .and_then(|mut dir| dir.next()?.ok())
    else {
        return;
    };
    let read = |file: &str| -> Option<i64> {
        std::fs::read_to_string(device.path().join(file)).ok()?.trim().parse().ok()
    };
    let (Some(current), Some(max)) = (read("brightness"), read("max_brightness")) else {
        return;
    };
    let step = (max / 10).max(1);
    // Never all the way to zero: on many panels that is a black screen.
    let target = if up { current + step } else { current - step }.clamp(step / 2, max);
    run(
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

impl Media {
    fn from_keysym(sym: Keysym) -> Option<Media> {
        Some(match sym {
            Keysym::XF86_AudioRaiseVolume => Media::VolumeUp,
            Keysym::XF86_AudioLowerVolume => Media::VolumeDown,
            Keysym::XF86_AudioMute => Media::Mute,
            Keysym::XF86_MonBrightnessUp => Media::BrightnessUp,
            Keysym::XF86_MonBrightnessDown => Media::BrightnessDown,
            _ => return None,
        })
    }

    fn apply(self) {
        const SINK: &str = "@DEFAULT_AUDIO_SINK@";
        match self {
            Media::VolumeUp => run("wpctl", &["set-volume", "-l", "1.0", SINK, "5%+"]),
            Media::VolumeDown => run("wpctl", &["set-volume", SINK, "5%-"]),
            Media::Mute => run("wpctl", &["set-mute", SINK, "toggle"]),
            Media::BrightnessUp => step_brightness(true),
            Media::BrightnessDown => step_brightness(false),
        }
    }
}

/// The keysyms that Ctrl+Alt+F1..F12 produce.
const SWITCH_VT_1: u32 = 0x1008_FE01;
const SWITCH_VT_12: u32 = 0x1008_FE0C;

impl EdexComp {
    /// Reads the input settings again and applies them to the keyboard and to every
    /// pointing device.
    pub fn reload_input(&mut self) {
        self.input_settings = khadi_common::input::InputSettings::load();
        let settings = self.input_settings.clone();
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.change_repeat_info(settings.repeat_rate as i32, settings.repeat_delay as i32);

        // Compiling a keymap is not free, and a bad one must not replace a good one.
        let wanted = (settings.keyboard_layout.clone(), settings.keyboard_variant.clone());
        let unset = wanted.0.is_empty() && self.keymap_applied.is_none();
        if !unset && self.keymap_applied.as_ref() != Some(&wanted) {
            let config = smithay::input::keyboard::XkbConfig {
                layout: &wanted.0,
                variant: &wanted.1,
                ..Default::default()
            };
            match keyboard.set_xkb_config(self, config) {
                Ok(()) => {
                    tracing::info!(layout = %wanted.0, variant = %wanted.1, "keyboard layout set");
                    self.keymap_applied = Some(wanted);
                }
                Err(e) => tracing::warn!(layout = %wanted.0, "cannot use this keyboard layout: {e:?}"),
            }
        }
        self.configure_devices();
    }

    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);

                let pressed = event.state() == KeyState::Pressed;
                let action = self.seat.get_keyboard().unwrap().input(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |_, modifiers, handle| {
                        // Super is the real shortcut key. Ctrl+Alt also works, because a
                        // host desktop keeps Super for itself when we run in a window.
                        let held = modifiers.logo || (modifiers.ctrl && modifiers.alt);
                        if !pressed {
                            return FilterResult::Forward;
                        }
                        let sym = handle.modified_sym();
                        if (SWITCH_VT_1..=SWITCH_VT_12).contains(&sym.raw()) {
                            let vt = (sym.raw() - SWITCH_VT_1 + 1) as i32;
                            return FilterResult::Intercept(Action::SwitchVt(vt));
                        }
                        if let Some(media) = Media::from_keysym(sym) {
                            return FilterResult::Intercept(Action::Media(media));
                        }
                        if !held {
                            return FilterResult::Forward;
                        }
                        match sym {
                            Keysym::BackSpace if modifiers.ctrl && modifiers.alt => {
                                FilterResult::Intercept(Action::Quit)
                            }
                            Keysym::Tab => FilterResult::Intercept(Action::Cycle),
                            Keysym::space => FilterResult::Intercept(Action::Launcher),
                            Keysym::comma => FilterResult::Intercept(Action::Settings),
                            Keysym::o | Keysym::O => FilterResult::Intercept(Action::MoveDisplay),
                            Keysym::R if modifiers.shift => FilterResult::Intercept(Action::RestartShell),
                            Keysym::s | Keysym::S => FilterResult::Intercept(Action::Split),
                            Keysym::f | Keysym::F => FilterResult::Intercept(Action::Fullscreen),
                            Keysym::Left => FilterResult::Intercept(Action::FocusHalf(false)),
                            Keysym::Right => FilterResult::Intercept(Action::FocusHalf(true)),
                            Keysym::bracketleft => {
                                FilterResult::Intercept(Action::Panel(khadi_common::Panel::Left))
                            }
                            Keysym::bracketright => {
                                FilterResult::Intercept(Action::Panel(khadi_common::Panel::Right))
                            }
                            Keysym::backslash => {
                                FilterResult::Intercept(Action::Panel(khadi_common::Panel::Bottom))
                            }
                            Keysym::q | Keysym::Q => FilterResult::Intercept(Action::Close),
                            _ => FilterResult::Forward,
                        }
                    },
                );
                match action {
                    Some(Action::Cycle) => self.cycle(),
                    Some(Action::Close) => self.close_active(),
                    Some(Action::MoveDisplay) => self.move_active(),
                    Some(Action::Launcher) => self.open_launcher(),
                    Some(Action::Settings) => self.open_settings(),
                    Some(Action::RestartShell) => self.restart_shell(),
                    Some(Action::Split) => self.toggle_split(),
                    Some(Action::Fullscreen) => self.toggle_fullscreen(),
                    Some(Action::FocusHalf(right)) => self.focus_half(right),
                    Some(Action::Panel(panel)) => self.toggle_panel(panel),
                    Some(Action::Media(media)) => media.apply(),
                    Some(Action::SwitchVt(vt)) => {
                        if let Some(udev) = self.udev.as_mut() {
                            if let Err(e) = udev.session.change_vt(vt) {
                                tracing::error!("cannot switch to VT {vt}: {e}");
                            }
                        }
                    }
                    Some(Action::Quit) => self.loop_signal.stop(),
                    None => {}
                }
            }
            // Mice and touchpads report movement; the pointer position is ours to keep.
            InputEvent::PointerMotion { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let from = pointer.current_location();
                let pos = self.clamp_to_screens(from, from + event.delta());

                let serial = SERIAL_COUNTER.next_serial();
                let under = self.surface_under(pos);
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output = self.space.outputs().next().unwrap();

                let output_geo = self.space.output_geometry(output).unwrap();

                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

                let serial = SERIAL_COUNTER.next_serial();

                let pointer = self.seat.get_pointer().unwrap();

                let under = self.surface_under(pos);

                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerButton { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let serial = SERIAL_COUNTER.next_serial();

                let button = event.button_code();

                let button_state = event.state();

                // Click to focus: the keyboard follows the display that was clicked.
                if ButtonState::Pressed == button_state && !pointer.is_grabbed() {
                    self.focus_at(pointer.current_location());
                };

                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: button_state,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event, .. } => {
                let source = event.source();

                let horizontal_amount = event
                    .amount(Axis::Horizontal)
                    .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.);
                let vertical_amount = event
                    .amount(Axis::Vertical)
                    .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.);
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                    if let Some(discrete) = horizontal_amount_discrete {
                        frame = frame.v120(Axis::Horizontal, discrete as i32);
                    }
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                    if let Some(discrete) = vertical_amount_discrete {
                        frame = frame.v120(Axis::Vertical, discrete as i32);
                    }
                }

                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                let pointer = self.seat.get_pointer().unwrap();
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }
}
