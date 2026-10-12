//! Wi-Fi, wired and VPN connections, and Bluetooth, for the settings screen. All are
//! driven through the system's own tools, `nmcli` and `bluetoothctl`, run on background
//! threads so that a slow scan never stalls the interface.

use crate::theme::Theme;
use crate::ui;
use eframe::egui::{self, Align, Align2, Key, Layout, Sense, Stroke, StrokeKind, TextEdit, Ui, vec2};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
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

/// A connection that is not Wi-Fi: a cable, or a VPN.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub vpn: bool,
    /// What it is called: the network device for a cable, the saved name for a VPN.
    pub name: String,
    pub state: LinkState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinkState {
    Up,
    Down,
    /// A wired port with no cable in it.
    Unplugged,
}

/// What a device being paired wants done.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// Type this code on the device itself, as keyboards ask.
    Type(String),
    /// Say whether the device shows this code, as phones ask.
    Confirm(String),
    /// Type here the code the device shows, or the one in its manual.
    Enter,
}

/// How pairing stands after more of `bluetoothctl`'s output.
#[derive(Debug, Clone, PartialEq)]
pub enum PairStep {
    Ask(Ask),
    /// A question that only ever has one answer here, which is to be given.
    Yes,
    Done,
    Failed(String),
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

/// Reads `nmcli -t -f TYPE,STATE,DEVICE device`: the wired ports.
pub fn parse_wired(output: &str) -> Vec<Link> {
    output
        .lines()
        .filter_map(|line| {
            let fields = split_terse(line, 3);
            let [kind, state, device] = fields.as_slice() else { return None };
            (kind == "ethernet" && !device.is_empty()).then(|| Link {
                vpn: false,
                name: device.clone(),
                state: if state.starts_with("connected") {
                    LinkState::Up
                } else if state == "unavailable" {
                    LinkState::Unplugged
                } else {
                    LinkState::Down
                },
            })
        })
        .collect()
}

/// Reads `nmcli -t -f TYPE,ACTIVE,NAME connection show`: the saved VPNs.
pub fn parse_vpns(output: &str) -> Vec<Link> {
    let mut vpns: Vec<Link> = output
        .lines()
        .filter_map(|line| {
            let fields = split_terse(line, 3);
            let [kind, active, name] = fields.as_slice() else { return None };
            (matches!(kind.as_str(), "vpn" | "wireguard") && !name.is_empty()).then(|| Link {
                vpn: true,
                name: name.clone(),
                state: if active == "yes" { LinkState::Up } else { LinkState::Down },
            })
        })
        .collect();
    vpns.sort_by_key(|vpn| (vpn.state != LinkState::Up, vpn.name.to_lowercase()));
    vpns
}

/// The kind of VPN a settings file is for, going by its name: what `nmcli connection
/// import` is to be told.
pub fn vpn_kind(path: &str) -> Option<&'static str> {
    let lower = path.to_lowercase();
    if lower.ends_with(".ovpn") {
        Some("openvpn")
    } else if lower.ends_with(".conf") {
        Some("wireguard")
    } else {
        None
    }
}

/// Takes the colour codes and line rewriting out of what a terminal program printed.
pub fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                if chars.next_if_eq(&'[').is_some() {
                    while chars.next().is_some_and(|c| !c.is_ascii_alphabetic()) {}
                }
            }
            '\r' => out.push('\n'),
            c => out.push(c),
        }
    }
    out
}

/// Finds the last thing of note in what `bluetoothctl` has printed while pairing.
/// `text` is already [`plain`].
pub fn pair_step(text: &str) -> Option<PairStep> {
    let code_after = |line: &str, lead: &str| {
        let rest = &line[line.find(lead)? + lead.len()..];
        let code: String = rest.trim_start().chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        (!code.is_empty()).then_some(code)
    };
    text.lines().rev().find_map(|line| {
        if line.contains("Pairing successful") || line.contains("AlreadyExists") {
            Some(PairStep::Done)
        } else if let Some(at) = line.find("Failed to pair:") {
            let why = line[at + "Failed to pair:".len()..].trim().trim_start_matches("org.bluez.Error.");
            Some(PairStep::Failed(if why.is_empty() { "the device refused".to_string() } else { why.to_string() }))
        } else if line.contains("not available") {
            Some(PairStep::Failed("the device is no longer in range".to_string()))
        } else if let Some(code) = code_after(line, "Confirm passkey") {
            Some(PairStep::Ask(Ask::Confirm(code)))
        } else if line.contains("Enter PIN code") || line.contains("Enter passkey") {
            Some(PairStep::Ask(Ask::Enter))
        } else if let Some(code) = code_after(line, "Passkey:").or_else(|| code_after(line, "PIN code:")) {
            Some(PairStep::Ask(Ask::Type(code)))
        } else if line.contains("(yes/no)") {
            Some(PairStep::Yes)
        } else {
            None
        }
    })
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

fn read_links() -> (Option<bool>, Vec<Link>) {
    let Ok(devices) = run("nmcli", &["-t", "-f", "TYPE,STATE,DEVICE", "device"]) else {
        return (None, Vec::new());
    };
    let mut links = parse_wired(&devices);
    links.extend(parse_vpns(&run("nmcli", &["-t", "-f", "TYPE,ACTIVE,NAME", "connection", "show"]).unwrap_or_default()));
    (Some(true), links)
}

/// Pairs with a device that may want a code, by holding a conversation with
/// `bluetoothctl`. What the device asks is put in `asking` for the settings to show;
/// answers come back through `answers`. Returns why it failed, if it did.
fn pair(program: &str, address: &str, asking: &Mutex<Option<Ask>>, answers: &Receiver<String>, ctx: &egui::Context) -> Result<(), String> {
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("cannot talk to bluetoothctl")?;
    let mut stdout = child.stdout.take().ok_or("cannot hear bluetoothctl")?;
    let (heard, hearing) = channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 1024];
        while let Ok(count) = stdout.read(&mut buffer) {
            if count == 0 || heard.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut say = |line: &str| stdin.write_all(format!("{line}\n").as_bytes()).and_then(|()| stdin.flush());
    let result = (|| {
        say("default-agent").and_then(|()| say(&format!("pair {address}"))).map_err(|e| format!("cannot talk to bluetoothctl: {e}"))?;
        let began = Instant::now();
        let mut said = String::new();
        // Only what comes after the last answer counts, or a question would be seen twice.
        let mut from = 0;
        loop {
            if began.elapsed() > Duration::from_secs(90) {
                return Err("it took too long".to_string());
            }
            if let Ok(answer) = answers.try_recv() {
                *asking.lock().unwrap() = None;
                ctx.request_repaint();
                if answer.is_empty() {
                    return Err("called off".to_string());
                }
                say(&answer).map_err(|e| format!("cannot talk to bluetoothctl: {e}"))?;
                from = said.len();
            }
            match hearing.recv_timeout(Duration::from_millis(150)) {
                Ok(bytes) => said.push_str(&plain(&String::from_utf8_lossy(&bytes))),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err("bluetoothctl stopped".to_string()),
            }
            match pair_step(&said[from..]) {
                Some(PairStep::Done) => break,
                Some(PairStep::Failed(why)) => return Err(why),
                Some(PairStep::Yes) => {
                    say("yes").map_err(|e| format!("cannot talk to bluetoothctl: {e}"))?;
                    from = said.len();
                }
                Some(PairStep::Ask(ask)) => {
                    let mut asking = asking.lock().unwrap();
                    if asking.as_ref() != Some(&ask) {
                        *asking = Some(ask);
                        ctx.request_repaint();
                    }
                }
                None => {}
            }
        }
        // Trusting it lets it reconnect by itself next time.
        let _ = say(&format!("trust {address}"));
        let _ = say(&format!("connect {address}"));
        // Connecting takes a moment, and there is no telling it apart from the rest.
        let _ = hearing.recv_timeout(Duration::from_secs(4));
        Ok(())
    })();
    *asking.lock().unwrap() = None;
    let _ = say("quit");
    drop(stdin);
    std::thread::sleep(Duration::from_millis(200));
    let _ = child.kill();
    let _ = child.wait();
    result
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
    links: Arc<Mutex<Shared<Link>>>,
    ctx: egui::Context,
    /// The network or device whose controls are open.
    selected: Option<String>,
    password: String,
    /// The VPN settings file being named, to bring in.
    vpn_file: String,
    /// What the device being paired is asking, and where the answer goes.
    asking: Arc<Mutex<Option<Ask>>>,
    answer: Option<Sender<String>>,
    code: String,
}

impl Radio {
    pub fn new(ctx: egui::Context) -> Radio {
        Radio {
            wifi: Arc::default(),
            bluetooth: Arc::default(),
            links: Arc::default(),
            ctx,
            selected: None,
            password: String::new(),
            vpn_file: String::new(),
            asking: Arc::default(),
            answer: None,
            code: String::new(),
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

    /// As [`Radio::wifi_do`], for cables and VPNs.
    fn links_do(&self, doing: &str, steps: Vec<Vec<String>>) {
        let shared = self.links.clone();
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
            let (powered, items) = read_links();
            *shared.lock().unwrap() = Shared { powered, items, working: None, problem, refreshed: Some(Instant::now()) };
            ctx.request_repaint();
        });
    }

    /// Pairs with a device, asking the user for whatever code it wants along the way.
    fn pair_with(&mut self, address: &str) {
        let shared = self.bluetooth.clone();
        let ctx = self.ctx.clone();
        {
            let mut state = shared.lock().unwrap();
            if state.working.is_some() {
                return;
            }
            state.working = Some("PAIRING".to_string());
            state.problem = None;
        }
        let (answer, answers) = channel();
        self.answer = Some(answer);
        self.code.clear();
        let asking = self.asking.clone();
        let address = address.to_string();
        std::thread::spawn(move || {
            let problem = pair("bluetoothctl", &address, &asking, &answers, &ctx).err().map(|why| format!("Not paired: {why}"));
            let (powered, items) = read_bluetooth();
            *shared.lock().unwrap() = Shared { powered, items, working: None, problem, refreshed: Some(Instant::now()) };
            ctx.request_repaint();
        });
    }

    /// Cables and VPNs, under the Wi-Fi networks.
    pub fn show_links(&mut self, ui: &mut Ui, theme: &Theme) {
        let (found, links, working, problem, stale) = {
            let state = self.links.lock().unwrap();
            let stale = state.refreshed.is_none_or(|at| at.elapsed() > Duration::from_secs(12));
            (state.powered, state.items.clone(), state.working.clone(), state.problem.clone(), stale)
        };
        if stale && working.is_none() {
            self.links_do("READING", Vec::new());
        }
        if found.is_none() {
            return;
        }
        let args = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<String>>();

        ui.add_space(26.0);
        ui::section(ui, "WIRED", |ui| {
            if let Some(doing) = &working {
                ui::meta(ui, doing, theme.main);
            }
        });
        let wired: Vec<&Link> = links.iter().filter(|link| !link.vpn).collect();
        if wired.is_empty() {
            ui::text(ui, "This machine has no wired port.", ui::mono(12.0), ui::LABEL);
        }
        for link in wired {
            let key = format!("wired {}", link.name);
            let open = self.selected.as_deref() == Some(&key);
            let state = match link.state {
                LinkState::Up => "CONNECTED",
                LinkState::Down => "NOT CONNECTED",
                LinkState::Unplugged => "NO CABLE",
            };
            if row(ui, theme, &link.name, "", state, link.state == LinkState::Up, open) {
                self.selected = if open { None } else { Some(key) };
            }
            if !open {
                continue;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(14.0);
                match link.state {
                    LinkState::Up => {
                        if ui::button(ui, theme, "DISCONNECT", None, false).clicked() {
                            self.links_do("DISCONNECTING", vec![args(&["device", "disconnect", &link.name])]);
                        }
                    }
                    LinkState::Down => {
                        if ui::button(ui, theme, "CONNECT", None, true).clicked() {
                            self.links_do("CONNECTING", vec![args(&["device", "connect", &link.name])]);
                        }
                    }
                    LinkState::Unplugged => {
                        ui::text(ui, "Plug a cable in and it connects by itself.", ui::mono(12.0), ui::LABEL);
                    }
                }
            });
            ui.add_space(10.0);
        }

        ui.add_space(26.0);
        ui::section(ui, "VPN", |_| {});
        let vpns: Vec<&Link> = links.iter().filter(|link| link.vpn).collect();
        if vpns.is_empty() {
            ui::text(ui, "No VPN is set up yet.", ui::mono(12.0), ui::LABEL);
        }
        for link in vpns {
            let key = format!("vpn {}", link.name);
            let open = self.selected.as_deref() == Some(&key);
            let up = link.state == LinkState::Up;
            if row(ui, theme, &link.name, "", if up { "CONNECTED" } else { "OFF" }, up, open) {
                self.selected = if open { None } else { Some(key) };
            }
            if !open {
                continue;
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add_space(14.0);
                if up {
                    if ui::button(ui, theme, "DISCONNECT", None, false).clicked() {
                        self.links_do("DISCONNECTING", vec![args(&["connection", "down", "id", &link.name])]);
                    }
                } else if ui::button(ui, theme, "CONNECT", None, true).clicked() {
                    self.links_do("CONNECTING", vec![args(&["connection", "up", "id", &link.name])]);
                }
                if ui::button(ui, theme, "FORGET", None, false).clicked() {
                    self.links_do("FORGETTING", vec![args(&["connection", "delete", "id", &link.name])]);
                }
            });
            ui.add_space(10.0);
        }
        if let Some(problem) = &problem {
            ui.add_space(8.0);
            ui::text(ui, problem, ui::mono(12.0), ui::CAUTION);
        }
        // Bringing one in from the file a VPN provider hands out.
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let go = field(ui, theme, &mut self.vpn_file, "path to a .conf (WireGuard) or .ovpn (OpenVPN) file", false, 420.0);
            let path = self.vpn_file.trim().to_string();
            let path = match path.strip_prefix("~/") {
                Some(rest) => format!("{}/{rest}", std::env::var("HOME").unwrap_or_default()),
                None => path,
            };
            let kind = vpn_kind(&path);
            if (ui::button(ui, theme, "ADD", None, kind.is_some()).clicked() || go)
                && let Some(kind) = kind
            {
                self.links_do("ADDING", vec![args(&["connection", "import", "type", kind, "file", &path])]);
                self.vpn_file.clear();
            }
        });
        ui.add_space(8.0);
        ui::text(ui, "A VPN that asks for a password each time needs it saved first.", ui::mono(12.0), ui::LABEL);
    }

    /// What the device being paired asks, with the means to answer. Returns whether
    /// there was anything to show.
    fn show_asking(&mut self, ui: &mut Ui, theme: &Theme) -> bool {
        let Some(ask) = self.asking.lock().unwrap().clone() else {
            return false;
        };
        let send = |answer: &str| {
            if let Some(to) = &self.answer {
                let _ = to.send(answer.to_string());
            }
        };
        let mut sent = false;
        ui::tile(ui, theme, true, |ui| {
            match &ask {
                Ask::Type(code) => {
                    ui::tracked(ui, "TYPE THIS ON THE DEVICE, THEN PRESS ITS ENTER KEY", ui::display_bold(12.0), ui::TEXT, 1.5);
                    ui.add_space(8.0);
                    ui::tracked(ui, code, ui::mono(28.0), theme.main, 6.0);
                }
                Ask::Confirm(code) => {
                    ui::tracked(ui, "DOES THE DEVICE SHOW THIS CODE?", ui::display_bold(12.0), ui::TEXT, 1.5);
                    ui.add_space(8.0);
                    ui::tracked(ui, code, ui::mono(28.0), theme.main, 6.0);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if ui::button(ui, theme, "YES, PAIR", None, true).clicked() {
                            send("yes");
                            sent = true;
                        }
                        if ui::button(ui, theme, "NO", None, false).clicked() {
                            send("no");
                            sent = true;
                        }
                    });
                }
                Ask::Enter => {
                    ui::tracked(ui, "TYPE THE CODE THE DEVICE SHOWS, OR THE ONE IN ITS MANUAL", ui::display_bold(12.0), ui::TEXT, 1.5);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let go = field(ui, theme, &mut self.code, "code", false, 160.0);
                        let ready = !self.code.trim().is_empty();
                        if (ui::button(ui, theme, "PAIR", None, ready).clicked() || go) && ready {
                            send(self.code.trim());
                            sent = true;
                        }
                    });
                }
            }
            ui.add_space(10.0);
            // An empty answer calls the whole thing off.
            if ui::button(ui, theme, "CANCEL", None, false).clicked() {
                send("");
                sent = true;
            }
        });
        if sent {
            self.code.clear();
            *self.asking.lock().unwrap() = None;
        }
        ui.add_space(14.0);
        true
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
                        go = field(ui, theme, &mut self.password, "password", true, 280.0);
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
        self.show_asking(ui, theme);
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
                    self.pair_with(address);
                }
                if device.paired && ui::button(ui, theme, "FORGET", None, false).clicked() {
                    self.bluetooth_do("FORGETTING", vec![args(&["remove", address])]);
                }
            });
            ui.add_space(10.0);
        }
    }
}

/// A one-line text field in the settings' style. Returns true when Enter was pressed in it.
fn field(ui: &mut Ui, theme: &Theme, text: &mut String, hint: &str, secret: bool, width: f32) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(width.min(ui.available_width()), 28.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, ui::PANEL);
    ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, theme.main), StrokeKind::Inside);
    let edit = ui.put(
        rect.shrink2(vec2(8.0, 4.0)),
        TextEdit::singleline(text).password(secret).frame(false).font(ui::mono(13.0)).text_color(ui::TEXT).hint_text(hint),
    );
    edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter))
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
    fn reads_cables_and_vpns() {
        let devices = "wifi:connected:wlp3s0\ntun:connected (externally):tailscale0\nethernet:unavailable:enp2s0f1\nethernet:connected:enx00\nethernet:disconnected:eth9\n";
        let wired = parse_wired(devices);
        let seen: Vec<(&str, LinkState)> = wired.iter().map(|link| (link.name.as_str(), link.state)).collect();
        assert_eq!(seen, [("enp2s0f1", LinkState::Unplugged), ("enx00", LinkState::Up), ("eth9", LinkState::Down)]);
        let saved = "802-11-wireless:yes:Home\nvpn:no:Work\nwireguard:yes:Away: the long way\nbridge:yes:docker0\n";
        let vpns = parse_vpns(saved);
        let seen: Vec<(&str, LinkState)> = vpns.iter().map(|link| (link.name.as_str(), link.state)).collect();
        assert_eq!(seen, [("Away: the long way", LinkState::Up), ("Work", LinkState::Down)]);
        assert_eq!(vpn_kind("/home/ada/Work.OVPN"), Some("openvpn"));
        assert_eq!(vpn_kind("wg0.conf"), Some("wireguard"));
        assert_eq!(vpn_kind("notes.txt"), None);
    }

    #[test]
    fn follows_a_pairing_conversation() {
        let said = plain("\u{1b}[0;94m[bluetoothctl]> \u{1b}[0m\r\u{1b}[KAttempting to pair with AA:BB:CC:DD:EE:FF\n");
        assert!(!said.contains('\u{1b}'));
        assert_eq!(pair_step(&said), None);
        assert_eq!(pair_step("[agent] Confirm passkey 123456 (yes/no): "), Some(PairStep::Ask(Ask::Confirm("123456".into()))));
        assert_eq!(pair_step("[agent] Passkey: 048213\n"), Some(PairStep::Ask(Ask::Type("048213".into()))));
        assert_eq!(pair_step("[agent] PIN code: 0000\n"), Some(PairStep::Ask(Ask::Type("0000".into()))));
        assert_eq!(pair_step("[agent] Enter PIN code: "), Some(PairStep::Ask(Ask::Enter)));
        assert_eq!(pair_step("[agent] Enter passkey (number in 0-999999): "), Some(PairStep::Ask(Ask::Enter)));
        assert_eq!(pair_step("[agent] Authorize service 0000110b (yes/no): "), Some(PairStep::Yes));
        assert_eq!(pair_step("[agent] Confirm passkey 123456 (yes/no): yes\nPairing successful\n"), Some(PairStep::Done));
        assert_eq!(
            pair_step("Failed to pair: org.bluez.Error.AuthenticationFailed\n"),
            Some(PairStep::Failed("AuthenticationFailed".into()))
        );
        assert_eq!(pair_step("Device AA:BB:CC:DD:EE:FF not available\n"), Some(PairStep::Failed("the device is no longer in range".into())));
    }

    /// Stands in for `bluetoothctl`: asks for a code to be confirmed, and pairs on a yes.
    const STAND_IN: &str = r#"#!/bin/sh
read agent; read pair
printf '\033[0;94m[bluetoothctl]> \033[0m\rAttempting to pair with %s\n' "${pair#pair }"
printf '[agent] Confirm passkey 482913 (yes/no): '
read answer
if [ "$answer" = yes ]; then echo 'Pairing successful'; else echo 'Failed to pair: org.bluez.Error.AuthenticationCanceled'; fi
while read line; do echo "said $line" >> "$0.log"; [ "$line" = quit ] && exit 0; done
"#;

    fn pair_through_stand_in(name: &str, answer: &str) -> (Result<(), String>, String) {
        use std::os::unix::fs::PermissionsExt;
        let script = std::env::temp_dir().join(format!("des-pair-{name}-{}.sh", std::process::id()));
        std::fs::write(&script, STAND_IN).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let asking = Arc::new(Mutex::new(None));
        let (answers, heard) = channel();
        let (watching, answer) = (asking.clone(), answer.to_string());
        // The user: waits to be asked, checks the code, answers.
        let user = std::thread::spawn(move || {
            for _ in 0..200 {
                if let Some(ask) = watching.lock().unwrap().clone() {
                    assert_eq!(ask, Ask::Confirm("482913".into()));
                    answers.send(answer).unwrap();
                    return answers;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("never asked");
        });
        let result = pair(script.to_str().unwrap(), "AA:BB:CC:DD:EE:FF", &asking, &heard, &egui::Context::default());
        let _answers = user.join().unwrap();
        assert_eq!(*asking.lock().unwrap(), None, "nothing is left asked");
        let log = script.with_extension("sh.log");
        let said = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_file(&log);
        (result, said)
    }

    #[test]
    fn pairs_once_the_code_is_confirmed() {
        let (result, said) = pair_through_stand_in("yes", "yes");
        assert_eq!(result, Ok(()));
        assert_eq!(said, "said trust AA:BB:CC:DD:EE:FF\nsaid connect AA:BB:CC:DD:EE:FF\nsaid quit\n");
    }

    #[test]
    fn a_refused_code_does_not_pair() {
        let (result, said) = pair_through_stand_in("no", "no");
        assert_eq!(result, Err("AuthenticationCanceled".to_string()));
        assert_eq!(said, "said quit\n");
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
