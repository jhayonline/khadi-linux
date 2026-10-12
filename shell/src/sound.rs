//! Sound: which device plays and which one listens, how loud each is, and how loud
//! each application is. All of it goes through `wpctl`, PipeWire's own tool, on
//! background threads, so that a slow answer never stalls the interface.

use crate::theme::Theme;
use crate::ui;
use eframe::egui::{self, Align2, Sense, Stroke, StrokeKind, Ui, vec2};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Something that plays, listens, or is an application making sound.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: u32,
    pub name: String,
    /// The one in use, among the outputs or the inputs.
    pub chosen: bool,
    /// Percent; `None` when it has not been read.
    pub volume: Option<u8>,
    pub muted: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Devices {
    pub outputs: Vec<Node>,
    pub inputs: Vec<Node>,
    /// Applications playing or recording something now.
    pub streams: Vec<Node>,
}

/// Reads a `[vol: 0.64]` or `[vol: 0.40 MUTED]` mark, or `Volume: 0.40 [MUTED]`.
pub fn parse_level(text: &str) -> (Option<u8>, bool) {
    let number = text
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|word| word.contains('.') && word.parse::<f32>().is_ok())
        .and_then(|word| word.parse::<f32>().ok());
    (number.map(|level| (level * 100.0).round().clamp(0.0, 150.0) as u8), text.contains("MUTED"))
}

/// Reads `wpctl status`: the outputs, inputs and streams of its audio part.
pub fn parse_status(status: &str) -> Devices {
    #[derive(PartialEq)]
    enum Part {
        Other,
        Outputs,
        Inputs,
        Streams,
    }
    let mut devices = Devices::default();
    let (mut audio, mut part) = (false, Part::Other);
    for line in status.lines() {
        // The parts are told apart by the lines that head them.
        let bare = line.trim_matches(|c: char| c.is_whitespace() || "│├└─".contains(c));
        match bare {
            "Audio" => {
                audio = true;
                continue;
            }
            "Video" | "Settings" => {
                audio = false;
                continue;
            }
            "Sinks:" => part = Part::Outputs,
            "Sources:" => part = Part::Inputs,
            "Streams:" => part = Part::Streams,
            "Devices:" | "Filters:" => part = Part::Other,
            _ => {}
        }
        if !audio || part == Part::Other || bare.ends_with(':') {
            continue;
        }
        // A stream's own lines are followed by those of its channels, which point somewhere.
        if bare.contains(" > ") {
            continue;
        }
        let (chosen, rest) = match bare.strip_prefix('*') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, bare),
        };
        let Some((id, rest)) = rest.split_once(". ") else { continue };
        let Ok(id) = id.trim().parse::<u32>() else { continue };
        let (name, level) = match rest.rfind("[vol:") {
            Some(at) => (&rest[..at], parse_level(&rest[at..])),
            None => (rest, (None, false)),
        };
        let node = Node { id, name: name.trim().to_string(), chosen, volume: level.0, muted: level.1 };
        if node.name.is_empty() {
            continue;
        }
        match part {
            Part::Outputs => devices.outputs.push(node),
            Part::Inputs => devices.inputs.push(node),
            Part::Streams => devices.streams.push(node),
            Part::Other => {}
        }
    }
    devices
}

fn wpctl(args: &[&str]) -> Option<String> {
    let output = Command::new("timeout").arg("5").arg("wpctl").args(args).stdin(std::process::Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read() -> Option<Devices> {
    let mut devices = parse_status(&wpctl(&["status"])?);
    // The list does not say how loud each application is; that is asked one by one.
    for stream in devices.streams.iter_mut().take(12) {
        if let Some(level) = wpctl(&["get-volume", &stream.id.to_string()]) {
            (stream.volume, stream.muted) = parse_level(&level);
        }
    }
    Some(devices)
}

struct Shared {
    /// `None` until first read, or when sound cannot be asked about.
    devices: Option<Devices>,
    read_at: Option<Instant>,
    reading: bool,
    /// Whether anything was ever read: tells "not yet" from "cannot".
    tried: bool,
}

/// The sound part of the settings.
pub struct Sound {
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    /// A level being dragged, held here until the system reports it back: the node,
    /// the level, and when.
    held: Option<(u32, u8, Instant)>,
}

impl Sound {
    pub fn new(ctx: egui::Context) -> Sound {
        Sound { shared: Arc::new(Mutex::new(Shared { devices: None, read_at: None, reading: false, tried: false })), ctx, held: None }
    }

    /// Does something with `wpctl`, if anything, then reads how things stand.
    fn act(&self, steps: Vec<Vec<String>>) {
        let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
        {
            let mut state = shared.lock().unwrap();
            if state.reading && steps.is_empty() {
                return;
            }
            state.reading = true;
        }
        std::thread::spawn(move || {
            for step in steps {
                let args: Vec<&str> = step.iter().map(String::as_str).collect();
                let _ = wpctl(&args);
            }
            let devices = read();
            let mut state = shared.lock().unwrap();
            state.devices = devices;
            state.read_at = Some(Instant::now());
            state.reading = false;
            state.tried = true;
            drop(state);
            ctx.request_repaint();
        });
    }

    /// The level to show for a node: the one being set, while that is fresh.
    fn level(&mut self, node: &Node) -> u8 {
        match self.held {
            Some((id, level, at)) if id == node.id && at.elapsed() < Duration::from_millis(1500) => level,
            _ => node.volume.unwrap_or(0),
        }
    }

    /// A slider for a node's level, with a way to silence it.
    fn level_row(&mut self, ui: &mut Ui, theme: &Theme, node: &Node, name: &str) {
        let level = self.level(node);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            // The slider takes what the name, the reading and the mute leave it.
            let wide = (ui.available_width() - 330.0).clamp(110.0, 260.0);
            let (label, _) = ui.allocate_exact_size(vec2(130.0, 24.0), Sense::hover());
            ui.painter().text(label.left_center(), Align2::LEFT_CENTER, crate::panels::truncate(name, 15), ui::mono(13.0), if node.muted { ui::LABEL } else { ui::TEXT });
            if let Some(frac) = ui::slider(ui, theme, wide, level.min(100) as f32 / 100.0) {
                let new = (frac * 100.0).round() as u8;
                if new != level {
                    self.held = Some((node.id, new, Instant::now()));
                    self.act(vec![vec!["set-volume".into(), "-l".into(), "1.0".into(), node.id.to_string(), format!("{:.2}", new as f32 / 100.0)]]);
                }
            }
            let (reading, _) = ui.allocate_exact_size(vec2(34.0, 24.0), Sense::hover());
            ui.painter().text(reading.left_center(), Align2::LEFT_CENTER, format!("{level}"), ui::mono(12.0), ui::TEXT);
            if ui::chip(ui, theme, if node.muted { "MUTED" } else { "MUTE" }, None, node.muted).clicked() {
                self.act(vec![vec!["set-mute".into(), node.id.to_string(), "toggle".into()]]);
            }
        });
        ui.add_space(6.0);
    }

    /// The devices of one kind, to choose among, and the level of the one in use.
    fn devices(&mut self, ui: &mut Ui, theme: &Theme, title: &str, none: &str, nodes: &[Node]) {
        ui::section(ui, title, |_| {});
        if nodes.is_empty() {
            ui::text(ui, none, ui::mono(12.0), ui::LABEL);
            ui.add_space(22.0);
            return;
        }
        for node in nodes {
            let state = if node.chosen { "IN USE" } else { "USE" };
            if row(ui, theme, &node.name, state, node.chosen) && !node.chosen {
                self.act(vec![vec!["set-default".into(), node.id.to_string()]]);
            }
        }
        ui.add_space(10.0);
        if let Some(chosen) = nodes.iter().find(|node| node.chosen) {
            self.level_row(ui, theme, chosen, "Level");
        }
        ui.add_space(16.0);
    }

    pub fn show(&mut self, ui: &mut Ui, theme: &Theme) {
        let (devices, stale, tried) = {
            let state = self.shared.lock().unwrap();
            (state.devices.clone(), state.read_at.is_none_or(|at| at.elapsed() > Duration::from_secs(3)), state.tried)
        };
        // Devices are plugged in and applications start and stop: keep looking.
        if stale {
            self.act(Vec::new());
            ui.ctx().request_repaint_after(Duration::from_secs(3));
        }
        let Some(devices) = devices else {
            ui::section(ui, "OUTPUT", |_| {});
            let text = if tried { "Sound cannot be asked about: wpctl, PipeWire's tool, gave no answer." } else { "Looking for sound devices…" };
            ui::text(ui, text, ui::mono(12.0), ui::LABEL);
            return;
        };
        self.devices(ui, theme, "OUTPUT", "No device to play through was found.", &devices.outputs);
        self.devices(ui, theme, "INPUT", "No microphone was found.", &devices.inputs);

        ui::section(ui, "APPLICATIONS", |_| {});
        if devices.streams.is_empty() {
            ui::text(ui, "Nothing is playing or recording at the moment.", ui::mono(12.0), ui::LABEL);
        }
        for stream in &devices.streams {
            let name = stream.name.clone();
            self.level_row(ui, theme, stream, &name);
        }
    }
}

/// One device in a list. Returns true when it was clicked.
fn row(ui: &mut Ui, theme: &Theme, name: &str, state: &str, live: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    let painter = ui.painter_at(rect);
    if live {
        painter.rect_filled(rect, 0.0, theme.tint());
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme.main), StrokeKind::Inside);
    } else {
        if response.hovered() {
            painter.rect_filled(rect, 0.0, ui::PANEL);
        }
        painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, ui::TRACK));
    }
    let mark = egui::Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(6.0, 6.0));
    painter.rect_filled(mark, 0.0, if live { theme.main } else { ui::LINE_STRONG });
    painter.text(rect.left_center() + vec2(30.0, 0.0), Align2::LEFT_CENTER, crate::panels::truncate(name, 48), ui::mono(13.0), ui::TEXT);
    let ink = if live { theme.main } else if response.hovered() { ui::TEXT } else { ui::LABEL };
    painter.text(rect.right_center() - vec2(14.0, 0.0), Align2::RIGHT_CENTER, state, ui::mono(11.0), ink);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "PipeWire 'pipewire-0' [1.6.2, ada@workstation, cookie:1]
 └─ Clients:
        33. pipewire                            [1.6.2, ada@workstation, pid:2315]

Audio
 ├─ Devices:
 │      55. Built-in Audio                      [alsa]
 │  
 ├─ Sinks:
 │  *   44. Built-in Audio Analog Stereo        [vol: 1.00]
 │      67. Kitchen Speaker                     [vol: 0.64 MUTED]
 │  
 ├─ Sources:
 │  *   54. Built-in Audio Analog Stereo        [vol: 0.27]
 │  
 ├─ Filters:
 │  
 └─ Streams:
        69. pw-cat                                                      
             72. output_FR       > ALC255 Analog:playback_FR\t[active]
             73. output_FL       > ALC255 Analog:playback_FL\t[active]
        81. Brave                                                       
             82. output_FL       > ALC255 Analog:playback_FL\t[active]

Video
 ├─ Sources:
 │  *   53. HD WebCam (V4L2)                   

Settings
 └─ Default Configured Devices:
         0. Audio/Sink    alsa_output.pci-0000_00_1f.3.hdmi-stereo
";

    #[test]
    fn reads_devices_and_streams() {
        let devices = parse_status(STATUS);
        assert_eq!(
            devices.outputs,
            [
                Node { id: 44, name: "Built-in Audio Analog Stereo".into(), chosen: true, volume: Some(100), muted: false },
                Node { id: 67, name: "Kitchen Speaker".into(), chosen: false, volume: Some(64), muted: true },
            ]
        );
        assert_eq!(devices.inputs.len(), 1, "the camera is no microphone");
        assert_eq!((devices.inputs[0].id, devices.inputs[0].volume), (54, Some(27)));
        let streams: Vec<(u32, &str)> = devices.streams.iter().map(|stream| (stream.id, stream.name.as_str())).collect();
        assert_eq!(streams, [(69, "pw-cat"), (81, "Brave")], "applications, not their channels");
        assert_eq!(parse_status(""), Devices::default());
    }

    #[test]
    fn reads_levels() {
        assert_eq!(parse_level("Volume: 0.55"), (Some(55), false));
        assert_eq!(parse_level("Volume: 0.40 [MUTED]"), (Some(40), true));
        assert_eq!(parse_level("[vol: 1.20]"), (Some(120), false));
        assert_eq!(parse_level("nothing"), (None, false));
    }
}
