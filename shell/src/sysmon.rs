//! Samples CPU, memory, process and network statistics once per second.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use sysinfo::{Networks, ProcessesToUpdate, System};

pub const HISTORY: usize = 60;
const TOP_PROCESSES: usize = 6;

pub struct ProcInfo {
    pub name: String,
    pub cpu: f32,
    pub mem: u64,
}

pub struct SysMon {
    sys: System,
    nets: Networks,
    last: Option<Instant>,
    pub host: String,
    pub os: String,
    pub kernel: String,
    /// Overall CPU usage in percent, oldest first.
    pub cpu_hist: VecDeque<f32>,
    pub cores: Vec<f32>,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub top: Vec<ProcInfo>,
    pub iface: String,
    pub ip: String,
    /// Download and upload rates in bytes per second, oldest first.
    pub rx_hist: VecDeque<f32>,
    pub tx_hist: VecDeque<f32>,
    pub rx_total: u64,
    pub tx_total: u64,
    pub battery: Option<u8>,
    /// Screen backlight, in percent of its range.
    pub brightness: Option<u8>,
    /// Wireless link quality, in percent.
    pub wifi: Option<u8>,
    /// Output volume in percent, sampled in the background.
    volume: Arc<AtomicU8>,
}

/// Stands for "not known" in the shared volume reading.
const NO_VOLUME: u8 = u8::MAX;

/// Where Khadi keeps the name it goes by, in preference to the base system's.
/// See `session/os-release` for why it is not /etc/os-release.
const KHADI_OS_RELEASE: [&str; 2] = [
    "/usr/local/share/khadi/os-release",
    "/usr/share/khadi/os-release",
];

/// The name of the operating system, for the system panel.
///
/// Khadi's own file first, then whatever the base system says. Reading Khadi's
/// rather than printing "Khadi" outright keeps the panel honest: the shell runs on
/// a plain Arch machine during development, through `dev.sh`, and there it should
/// say Arch — nothing has been installed that would make it anything else.
fn os_name() -> String {
    KHADI_OS_RELEASE
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| os_release_name(&text))
        .or_else(System::name)
        .unwrap_or_else(|| "Linux".into())
}

/// Strips one layer of matching quotes, which os-release values usually carry.
fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(rest) = value.strip_prefix(quote)
            && let Some(inner) = rest.strip_suffix(quote)
        {
            return inner;
        }
    }
    value
}

/// `NAME=` from an os-release(5) file. Values may be quoted, and `#` starts a
/// comment, which this file has at the top of it.
fn os_release_name(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some(value) = line.strip_prefix("NAME=") else {
            continue;
        };
        let value = value.trim();
        let value = unquote(value);
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

impl SysMon {
    pub fn new() -> SysMon {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        SysMon {
            sys,
            nets: Networks::new_with_refreshed_list(),
            last: None,
            host: System::host_name().unwrap_or_else(|| "localhost".into()),
            os: os_name(),
            kernel: System::kernel_version().unwrap_or_default(),
            cpu_hist: VecDeque::from(vec![0.0; HISTORY]),
            cores: Vec::new(),
            mem_used: 0,
            mem_total: 0,
            swap_used: 0,
            swap_total: 0,
            top: Vec::new(),
            iface: String::new(),
            ip: String::new(),
            rx_hist: VecDeque::from(vec![0.0; HISTORY]),
            tx_hist: VecDeque::from(vec![0.0; HISTORY]),
            rx_total: 0,
            tx_total: 0,
            battery: None,
            brightness: None,
            wifi: None,
            volume: watch_volume(),
        }
    }

    /// Takes a new sample if at least a second has passed since the last one.
    pub fn tick(&mut self) {
        let elapsed = match self.last {
            Some(last) if last.elapsed() < Duration::from_secs(1) => return,
            Some(last) => last.elapsed().as_secs_f32(),
            None => 1.0,
        };
        self.last = Some(Instant::now());

        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.nets.refresh(true);

        push(&mut self.cpu_hist, self.sys.global_cpu_usage());
        self.cores = self.sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        self.mem_used = self.sys.used_memory();
        self.mem_total = self.sys.total_memory();
        self.swap_used = self.sys.used_swap();
        self.swap_total = self.sys.total_swap();

        let ncores = self.cores.len().max(1) as f32;
        let mut procs: Vec<ProcInfo> = self
            .sys
            .processes()
            .values()
            // Threads show up as processes on Linux; only keep thread-group leaders.
            .filter(|p| p.thread_kind().is_none())
            .map(|p| ProcInfo {
                name: p.name().to_string_lossy().into_owned(),
                cpu: p.cpu_usage() / ncores,
                mem: p.memory(),
            })
            .collect();
        procs.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.mem.cmp(&a.mem)));
        procs.truncate(TOP_PROCESSES);
        self.top = procs;

        // Report on the busiest real interface.
        let active = self
            .nets
            .list()
            .iter()
            .filter(|(name, _)| name.as_str() != "lo")
            .max_by_key(|(_, data)| data.total_received() + data.total_transmitted());
        match active {
            Some((name, data)) => {
                self.iface = name.clone();
                self.ip = data
                    .ip_networks()
                    .iter()
                    .find(|n| n.addr.is_ipv4())
                    .map(|n| n.addr.to_string())
                    .unwrap_or_default();
                push(&mut self.rx_hist, data.received() as f32 / elapsed);
                push(&mut self.tx_hist, data.transmitted() as f32 / elapsed);
                self.rx_total = data.total_received();
                self.tx_total = data.total_transmitted();
            }
            None => {
                self.iface.clear();
                self.ip.clear();
                push(&mut self.rx_hist, 0.0);
                push(&mut self.tx_hist, 0.0);
            }
        }

        self.battery = read_battery();
        self.brightness = read_brightness();
        self.wifi = read_wifi();
    }

    pub fn volume(&self) -> Option<u8> {
        match self.volume.load(Ordering::Relaxed) {
            NO_VOLUME => None,
            volume => Some(volume),
        }
    }

    pub fn uptime() -> u64 {
        System::uptime()
    }
}

fn push(hist: &mut VecDeque<f32>, value: f32) {
    hist.push_back(value);
    while hist.len() > HISTORY {
        hist.pop_front();
    }
}

/// Asks the audio server for the output volume every two seconds, on a thread of its
/// own because that means running a program.
fn watch_volume() -> Arc<AtomicU8> {
    let shared = Arc::new(AtomicU8::new(NO_VOLUME));
    let writer = shared.clone();
    std::thread::spawn(move || {
        // Stops once the monitor that reads this is gone.
        while Arc::strong_count(&writer) > 1 {
            let reading = std::process::Command::new("wpctl")
                .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
                .output()
                .ok()
                .and_then(|output| parse_volume(&String::from_utf8_lossy(&output.stdout)));
            writer.store(reading.unwrap_or(NO_VOLUME), Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    shared
}

/// Reads `wpctl get-volume` output such as `Volume: 0.55` or `Volume: 0.40 [MUTED]`.
fn parse_volume(output: &str) -> Option<u8> {
    if output.contains("MUTED") {
        return Some(0);
    }
    let level: f32 = output.split_whitespace().nth(1)?.parse().ok()?;
    Some((level * 100.0).round().clamp(0.0, 200.0) as u8)
}

fn read_brightness() -> Option<u8> {
    let device = std::fs::read_dir("/sys/class/backlight").ok()?.flatten().next()?.path();
    let read = |file: &str| -> Option<f32> {
        std::fs::read_to_string(device.join(file)).ok()?.trim().parse().ok()
    };
    let (current, max) = (read("brightness")?, read("max_brightness")?);
    (max > 0.0).then(|| (current / max * 100.0).round() as u8)
}

/// Link quality of the first wireless interface, from the kernel's table (out of 70).
fn read_wifi() -> Option<u8> {
    let table = std::fs::read_to_string("/proc/net/wireless").ok()?;
    let line = table.lines().nth(2)?;
    let quality: f32 = line.split_whitespace().nth(2)?.trim_end_matches('.').parse().ok()?;
    Some((quality / 70.0 * 100.0).round().clamp(0.0, 100.0) as u8)
}

fn read_battery() -> Option<u8> {
    for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let path = entry.path();
        let is_battery = std::fs::read_to_string(path.join("type"))
            .map(|t| t.trim() == "Battery")
            .unwrap_or(false);
        if is_battery {
            if let Ok(cap) = std::fs::read_to_string(path.join("capacity")) {
                return cap.trim().parse().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_volume() {
        assert_eq!(parse_volume("Volume: 0.55\n"), Some(55));
        assert_eq!(parse_volume("Volume: 0.40 [MUTED]\n"), Some(0));
        assert_eq!(parse_volume("Volume: 1.25"), Some(125));
        assert_eq!(parse_volume(""), None);
    }

    #[test]
    fn samples_have_sane_values() {
        let mut mon = SysMon::new();
        mon.tick();
        assert!(mon.mem_total > 0);
        assert!(mon.mem_used <= mon.mem_total);
        assert!(!mon.cores.is_empty());
        assert!(!mon.top.is_empty());
        assert_eq!(mon.cpu_hist.len(), HISTORY);
    }

    #[test]
    fn reads_the_name_out_of_os_release() {
        let text = "# a comment\nID=khadi\nNAME=\"Khadi Linux\"\nPRETTY_NAME=\"Khadi Linux\"\n";
        assert_eq!(os_release_name(text).as_deref(), Some("Khadi Linux"));
    }

    #[test]
    fn takes_name_unquoted_and_ignores_the_rest() {
        assert_eq!(os_release_name("NAME=Khadi\n").as_deref(), Some("Khadi"));
        assert_eq!(os_release_name("NAME='Khadi Linux'\n").as_deref(), Some("Khadi Linux"));
        // PRETTY_NAME ends in NAME= but is not it, and must not be mistaken for it.
        assert_eq!(os_release_name("PRETTY_NAME=\"Arch\"\nNAME=\"Khadi\"\n").as_deref(), Some("Khadi"));
        // Nothing to report rather than an empty string, so the caller falls back.
        assert_eq!(os_release_name("ID=khadi\n"), None);
        assert_eq!(os_release_name("NAME=\"\"\n"), None);
    }
}
