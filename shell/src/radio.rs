//! Wi-Fi and Bluetooth, for the settings screen. Both are driven through the system's
//! own tools, `nmcli` and `bluetoothctl`, run on background threads so that a slow scan
//! never stalls the interface.

use crate::theme::Theme;
use crate::ui;
use eframe::egui::{self, Align, Align2, Key, Layout, Sense, Stroke, StrokeKind, TextEdit, Ui, vec2};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// Signal strength in percent.
    pub signal: u8,
    pub secured: bool,
    pub in_use: bool,
    /// Whether its password is already saved.
    pub known: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BtDevice {
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
}

/// What is known about a radio, and what is being done with it.
struct Shared<T> {
    /// `None` until first read, or when the machine has no such radio.
    powered: Option<bool>,
    items: Vec<T>,
    /// The action in progress, in words for the user.
    working: Option<String>,
    /// How the last action ended, when it failed.
    problem: Option<String>,
    refreshed: Option<Instant>,
}

impl<T> Default for Shared<T> {
    fn default() -> Self {
        Shared {
            powered: None,
            items: Vec::new(),
            working: None,
            problem: None,
            refreshed: None,
        }
    }
}

/// Splits one line of `nmcli --terse` output, where `\:` is a colon inside a field.
/// The last field takes whatever remains, colons included.
pub fn split_terse(line: &str, fields: usize) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    parts.last_mut().unwrap().push(next);
                }
            }
            ':' if parts.len() < fields => parts.push(String::new()),
            c => parts.last_mut().unwrap().push(c),
        }
    }
    parts
}

/// Reads `nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID device wifi list`. A network heard
/// through several access points is listed once, at its strongest.
pub fn parse_networks(output: &str, known: &[String]) -> Vec<Network> {
    let mut networks: Vec<Network> = Vec::new();
    for line in output.lines() {
        let fields = split_terse(line, 4);
        let [in_use, signal, security, ssid] = fields.as_slice() else {
            continue;
        };
        if ssid.is_empty() {
            continue;
        }
        let network = Network {
            ssid: ssid.clone(),
            signal: signal.parse().unwrap_or(0),
            secured: !security.is_empty() && security != "--",
            in_use: in_use == "*",
            known: known.contains(ssid),
        };
        match networks.iter_mut().find(|other| other.ssid == network.ssid) {
            Some(other) => {
                other.in_use |= network.in_use;
                other.signal = other.signal.max(network.signal);
            }
            None => networks.push(network),
        }
    }
    // The one in use first, then saved ones, then by strength.
    networks.sort_by_key(|n| (!n.in_use, !n.known, std::cmp::Reverse(n.signal)));
    networks
}

/// Reads `bluetoothctl devices`: lines of `Device <address> <name>`.
pub fn parse_devices(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("Device ")?;
            let (address, name) = rest.split_once(' ').unwrap_or((rest, ""));
            let valid = address.len() == 17 && address.chars().all(|c| c.is_ascii_hexdigit() || c == ':');
            valid.then(|| (address.to_string(), name.to_string()))
        })
        .collect()
}

/// Reads a `Key: yes` line from `bluetoothctl show` or `bluetoothctl info`.
pub fn flag(output: &str, key: &str) -> Option<bool> {
    output.lines().find_map(|line| {
        let value = line.trim().strip_prefix(key)?.strip_prefix(':')?.trim();
        Some(value == "yes")
    })
}

/// Runs a tool and returns what it printed, or the first line of its complaint.
fn run(program: &str, args: &[&str]) -> Result<String, String> {
    // Neither tool should take long; a hung one must not hang a thread for good.
    let output = Command::new("timeout")
        .arg("30")
        .arg(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_string();
    if output.status.success() {
        Ok(text(&output.stdout))
    } else {
        let complaint = [text(&output.stderr), text(&output.stdout)].into_iter().find(|t| !t.is_empty());
        Err(complaint.and_then(|t| t.lines().next().map(str::to_string)).unwrap_or_else(|| format!("{program} failed")))
    }
}

fn read_wifi(rescan: bool) -> (Option<bool>, Vec<Network>) {
    let powered = run("nmcli", &["-t", "-f", "WIFI", "radio"]).ok().map(|state| state == "enabled");
    if powered != Some(true) {
        return (powered, Vec::new());
    }
    let known: Vec<String> = run("nmcli", &["-t", "-f", "TYPE,NAME", "connection", "show"])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let fields = split_terse(line, 2);
            (fields.first().map(String::as_str) == Some("802-11-wireless")).then(|| fields.get(1).cloned())?
        })
        .collect();
    let rescan = if rescan { "yes" } else { "no" };
    let list = run("nmcli", &["-t", "-f", "IN-USE,SIGNAL,SECURITY,SSID", "device", "wifi", "list", "--rescan", rescan]);
    (powered, parse_networks(&list.unwrap_or_default(), &known))
}

fn read_bluetooth() -> (Option<bool>, Vec<BtDevice>) {
    let powered = run("bluetoothctl", &["show"]).ok().and_then(|show| flag(&show, "Powered"));
    if powered != Some(true) {
        return (powered, Vec::new());
    }
    let mut devices: Vec<BtDevice> = parse_devices(&run("bluetoothctl", &["devices"]).unwrap_or_default())
        .into_iter()
        .take(24)
        .map(|(address, name)| {
            let info = run("bluetoothctl", &["info", &address]).unwrap_or_default();
            BtDevice {
                // A device that never said its name shows its address instead.
                name: if name.is_empty() || name.replace('-', ":") == address { address.clone() } else { name },
                paired: flag(&info, "Paired").unwrap_or(false),
                connected: flag(&info, "Connected").unwrap_or(false),
                address,
            }
        })
        .collect();
    devices.sort_by_key(|d| (!d.connected, !d.paired, d.name.to_lowercase()));
    (powered, devices)
}

/// The Wi-Fi and Bluetooth panels of the settings screen.
pub struct Radio {
    wifi: Arc<Mutex<Shared<Network>>>,
    bluetooth: Arc<Mutex<Shared<BtDevice>>>,
    ctx: egui::Context,
    /// The network or device whose controls are open.
    selected: Option<String>,
    password: String,
}

impl Radio {
    pub fn new(ctx: egui::Context) -> Radio {
        Radio {
            wifi: Arc::default(),
            bluetooth: Arc::default(),
            ctx,
            selected: None,
            password: String::new(),
        }
    }

    /// Runs `steps` in the background while showing `doing`, then reads the networks
    /// again. A step that fails stops the rest and is reported.
    fn wifi_do(&self, doing: &str, steps: Vec<Vec<String>>, rescan: bool) {
        let shared = self.wifi.clone();
        let ctx = self.ctx.clone();
        {
            let mut state = shared.lock().unwrap();
            if state.working.is_some() {
                return;
            }
            state.working = Some(doing.to_string());
            state.problem = None;
        }
        std::thread::spawn(move || {
            let mut problem = None;
            for step in steps {
                let args: Vec<&str> = step.iter().map(String::as_str).collect();
                if let Err(e) = run("nmcli", &args) {
                    problem = Some(e);
                    break;
                }
            }
            let (powered, items) = read_wifi(rescan);
            let mut state = shared.lock().unwrap();
            *state = Shared {
                powered,
                items,
                working: None,
                problem,
                refreshed: Some(Instant::now()),
            };
            ctx.request_repaint();
        });
    }

    fn bluetooth_do(&self, doing: &str, steps: Vec<Vec<String>>) {
        let shared = self.bluetooth.clone();
        let ctx = self.ctx.clone();
        {
            let mut state = shared.lock().unwrap();
            if state.working.is_some() {
                return;
            }
            state.working = Some(doing.to_string());
            state.problem = None;
        }
        std::thread::spawn(move || {
            let mut problem = None;
            for step in steps {
                let args: Vec<&str> = step.iter().map(String::as_str).collect();
                if let Err(e) = run("bluetoothctl", &args) {
                    problem = Some(e);
                    break;
                }
            }
            let (powered, items) = read_bluetooth();
            let mut state = shared.lock().unwrap();
            *state = Shared {
                powered,
                items,
                working: None,
                problem,
                refreshed: Some(Instant::now()),
            };
            ctx.request_repaint();
        });
    }

    pub fn show_wifi(&mut self, ui: &mut Ui, theme: &Theme) {
        let (powered, networks, working, problem, stale) = {
            let state = self.wifi.lock().unwrap();
            let stale = state.refreshed.is_none_or(|at| at.elapsed() > Duration::from_secs(12));
            (state.powered, state.items.clone(), state.working.clone(), state.problem.clone(), stale)
        };
        if stale && working.is_none() {
            self.wifi_do("READING", Vec::new(), false);
        }
        let args = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<String>>();

        ui::section(ui, "WI-FI", |ui| {
            if let Some(doing) = &working {
                ui::meta(ui, doing, theme.main);
            }
        });
        let Some(on) = powered else {
            let text = if working.is_some() { "Looking for the Wi-Fi radio…" } else { "No Wi-Fi radio was found on this machine." };
            ui::text(ui, text, ui::mono(12.0), ui::LABEL);
            return;
        };
        if switch(ui, theme, "Wi-Fi", if on { "On" } else { "Off: no wireless connections" }, on) {
            let next = if on { "off" } else { "on" };
            self.wifi_do(if on { "TURNING OFF" } else { "TURNING ON" }, vec![args(&["radio", "wifi", next])], !on);
        }
        if let Some(problem) = &problem {
            ui.add_space(8.0);
            ui::text(ui, problem, ui::mono(12.0), ui::CAUTION);
        }
        if !on {
            return;
        }
        ui.add_space(18.0);
        ui::section(ui, "NETWORKS", |ui| {
            if ui::button(ui, theme, "SCAN", None, false).clicked() {
                self.wifi_do("SCANNING", Vec::new(), true);
            }
        });
        if networks.is_empty() {
            ui::text(ui, "No networks in range yet.", ui::mono(12.0), ui::LABEL);
        }
        for network in &networks {
            let open = self.selected.as_deref() == Some(&network.ssid);
            let state = match (network.in_use, network.known) {
                (true, _) => "CONNECTED",
                (false, true) => "SAVED",
                (false, false) if network.secured => "SECURED",
                (false, false) => "OPEN",
            };
            if row(ui, theme, &network.ssid, &format!("{}%", network.signal), state, network.in_use, open) {
                self.selected = if open { None } else { Some(network.ssid.clone()) };
                self.password.clear();
            }
            if !open {
                continue;
            }
            // The chosen network's controls, directly under it.
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(14.0);
                let ssid = network.ssid.as_str();
                if network.in_use {
                    if ui::button(ui, theme, "DISCONNECT", None, false).clicked() {
                        self.wifi_do("DISCONNECTING", vec![args(&["connection", "down", "id", ssid])], false);
                    }
                } else if network.known {
                    if ui::button(ui, theme, "CONNECT", None, true).clicked() {
                        self.wifi_do("CONNECTING", vec![args(&["connection", "up", "id", ssid])], false);
                    }
                } else {
                    let mut go = false;
                    if network.secured {
                        let (field, _) = ui.allocate_exact_size(vec2(280.0, 28.0), Sense::hover());
                        ui.painter().rect_filled(field, 0.0, ui::PANEL);
                        ui.painter().rect_stroke(field, 0.0, Stroke::new(1.0, theme.main), StrokeKind::Inside);
                        let edit = ui.put(
                            field.shrink2(vec2(8.0, 4.0)),
                            TextEdit::singleline(&mut self.password)
                                .password(true)
                                .frame(false)
                                .font(ui::mono(13.0))
                                .text_color(ui::TEXT)
                                .hint_text("password"),
                        );
                        go = edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                    }
                    let ready = !network.secured || self.password.len() >= 8;
                    go |= ui::button(ui, theme, "CONNECT", None, ready).clicked();
                    if go && ready {
                        let mut step = args(&["device", "wifi", "connect", ssid]);
                        if network.secured {
                            step.extend(args(&["password", &self.password]));
                        }
                        self.password.clear();
                        self.wifi_do("CONNECTING", vec![step], false);
                    }
                }
                if network.known && ui::button(ui, theme, "FORGET", None, false).clicked() {
                    self.wifi_do("FORGETTING", vec![args(&["connection", "delete", "id", ssid])], false);
                }
            });
            ui.add_space(10.0);
        }
    }

    pub fn show_bluetooth(&mut self, ui: &mut Ui, theme: &Theme) {
        let (powered, devices, working, problem, stale) = {
            let state = self.bluetooth.lock().unwrap();
            let stale = state.refreshed.is_none_or(|at| at.elapsed() > Duration::from_secs(12));
            (state.powered, state.items.clone(), state.working.clone(), state.problem.clone(), stale)
        };
        if stale && working.is_none() {
            self.bluetooth_do("READING", Vec::new());
        }
        let args = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<String>>();

        ui::section(ui, "BLUETOOTH", |ui| {
            if let Some(doing) = &working {
                ui::meta(ui, doing, theme.main);
            }
        });
        let Some(on) = powered else {
            let text = if working.is_some() { "Looking for the Bluetooth radio…" } else { "No Bluetooth radio was found on this machine." };
            ui::text(ui, text, ui::mono(12.0), ui::LABEL);
            return;
        };
        if switch(ui, theme, "Bluetooth", if on { "On" } else { "Off" }, on) {
            let next = if on { "off" } else { "on" };
            self.bluetooth_do(if on { "TURNING OFF" } else { "TURNING ON" }, vec![args(&["power", next])]);
        }
        if let Some(problem) = &problem {
            ui.add_space(8.0);
            ui::text(ui, problem, ui::mono(12.0), ui::CAUTION);
        }
        if !on {
            return;
        }
        ui.add_space(18.0);
        ui::section(ui, "DEVICES", |ui| {
            if ui::button(ui, theme, "SCAN", None, false).clicked() {
                // Listening for eight seconds finds what is in pairing mode nearby.
                self.bluetooth_do("SCANNING FOR 8 SECONDS", vec![args(&["--timeout", "8", "scan", "on"])]);
            }
        });
        if devices.is_empty() {
            ui::text(ui, "No devices yet. Put one in pairing mode and scan.", ui::mono(12.0), ui::LABEL);
        }
        for device in &devices {
            let open = self.selected.as_deref() == Some(&device.address);
            let state = match (device.connected, device.paired) {
                (true, _) => "CONNECTED",
                (false, true) => "PAIRED",
                (false, false) => "NEW",
            };
            if row(ui, theme, &device.name, "", state, device.connected, open) {
                self.selected = if open { None } else { Some(device.address.clone()) };
            }
            if !open {
                continue;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(14.0);
                let address = device.address.as_str();
                if device.connected {
                    if ui::button(ui, theme, "DISCONNECT", None, false).clicked() {
                        self.bluetooth_do("DISCONNECTING", vec![args(&["disconnect", address])]);
                    }
                } else if device.paired {
                    if ui::button(ui, theme, "CONNECT", None, true).clicked() {
                        self.bluetooth_do("CONNECTING", vec![args(&["connect", address])]);
                    }
                } else if ui::button(ui, theme, "PAIR", None, true).clicked() {
                    // Trusting it lets it reconnect by itself next time.
                    let steps = vec![args(&["pair", address]), args(&["trust", address]), args(&["connect", address])];
                    self.bluetooth_do("PAIRING", steps);
                }
                if device.paired && ui::button(ui, theme, "FORGET", None, false).clicked() {
                    self.bluetooth_do("FORGETTING", vec![args(&["remove", address])]);
                }
            });
            ui.add_space(10.0);
        }
        ui.add_space(14.0);
        ui::text(ui, "Devices that ask for a code to be typed, such as some keyboards, cannot be paired here yet.", ui::mono(12.0), ui::LABEL);
    }
}

/// A named on/off switch on one line. Returns true when it was clicked.
fn switch(ui: &mut Ui, theme: &Theme, name: &str, what: &str, on: bool) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        ui::text(ui, name, ui::mono(13.0), ui::TEXT);
        ui::text(ui, what, ui::mono(12.0), ui::LABEL);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            clicked = ui::toggle(ui, theme, on).clicked();
        });
    });
    clicked
}

/// One network or device in a list. Returns true when it was clicked.
fn row(ui: &mut Ui, theme: &Theme, name: &str, reading: &str, state: &str, live: bool, open: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let painter = ui.painter_at(rect);
    if open {
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
    painter.text(rect.left_center() + vec2(30.0, 0.0), Align2::LEFT_CENTER, crate::panels::truncate(name, 40), ui::mono(13.0), ui::TEXT);
    painter.text(rect.right_center() - vec2(14.0, 0.0), Align2::RIGHT_CENTER, state, ui::mono(11.0), if live { theme.main } else { ui::LABEL });
    painter.text(rect.right_center() - vec2(110.0, 0.0), Align2::RIGHT_CENTER, reading, ui::mono(12.0), ui::SECONDARY);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_terse_lines() {
        assert_eq!(split_terse("*:78:WPA2:Home", 4), ["*", "78", "WPA2", "Home"]);
        assert_eq!(split_terse(r" :40::Cafe\: upstairs", 4), [" ", "40", "", "Cafe: upstairs"]);
        assert_eq!(split_terse("802-11-wireless:My:Net", 2), ["802-11-wireless", "My:Net"]);
        assert_eq!(split_terse(r"a\\b:c", 2), [r"a\b", "c"]);
    }

    #[test]
    fn reads_networks() {
        let output = " :40:WPA2:Neighbour\n*:78:WPA1 WPA2:Home\n :55:WPA2:Home\n :30::Cafe\n :20:WPA2:\n :61:--:Saved Open\n";
        let networks = parse_networks(output, &["Home".to_string(), "Saved Open".to_string()]);
        let names: Vec<&str> = networks.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["Home", "Saved Open", "Neighbour", "Cafe"], "in use, then saved, then by strength");
        assert_eq!(networks[0], Network { ssid: "Home".into(), signal: 78, secured: true, in_use: true, known: true });
        assert!(!networks[1].secured && networks[1].known);
        assert!(!networks[3].secured && !networks[3].known);
    }

    #[test]
    fn reads_bluetooth_output() {
        let devices = parse_devices("Device AA:BB:CC:DD:EE:FF WH-1000XM4\nDevice 11:22:33:44:55:66 My Mouse 2\n[NEW] something else\nDevice bad name\n");
        assert_eq!(devices, [("AA:BB:CC:DD:EE:FF".to_string(), "WH-1000XM4".to_string()), ("11:22:33:44:55:66".to_string(), "My Mouse 2".to_string())]);
        let show = "Controller 00:11:22:33:44:55 (public)\n\tName: laptop\n\tPowered: yes\n\tDiscoverable: no\n";
        assert_eq!(flag(show, "Powered"), Some(true));
        assert_eq!(flag(show, "Discoverable"), Some(false));
        assert_eq!(flag(show, "Pairable"), None);
    }
}
