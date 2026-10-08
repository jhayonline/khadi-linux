//! System metrics.
//!
//! Deliberately narrow: exactly what the eDEX panels show, nothing else.
//! `sysinfo` is the portable floor; anything it cannot give us that Linux can
//! is read from /proc or /sys directly and marked as such.

use std::{collections::VecDeque, fs, time::Duration};
use sysinfo::{System, RefreshKind, CpuRefreshKind, MemoryRefreshKind};

/// How much CPU history the sparklines keep. eDEX shows a rolling window per
/// core pair; 60 samples at 1s is a minute, which fits a 24-cell sparkline
/// with room to downsample.
const HISTORY: usize = 60;

#[derive(Debug, Clone, Default)]
pub struct Hardware {
    pub vendor: String,
    pub model: String,
    pub chassis: String,
    pub cpu_model: String,
    pub cores: usize,
    pub host: String,
}

#[derive(Debug, Clone, Default)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub cpu: f32,
    pub mem: u64,
}

pub struct Metrics {
    sys: System,
    pub hardware: Hardware,
    pub per_core: Vec<VecDeque<f32>>,
    pub memory: Memory,
    pub procs: Vec<Process>,
    pub temp_c: Option<f32>,
    pub tasks: usize,
    pub uptime: Duration,
}

/// CPU brand strings are full of noise the panel has no room for:
/// "Intel(R) Core(TM) i7-7500U CPU @ 2.70GHz" in a 34-column header leaves
/// nothing for the label. eDEX shows "Intel Core i5-4200H".
fn clean_cpu(brand: &str) -> String {
    let mut s = brand.to_string();
    for noise in ["(R)", "(TM)", "(r)", "(tm)", " CPU", " Processor"] {
        s = s.replace(noise, "");
    }
    if let Some(at) = s.find('@') {
        s.truncate(at);
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// SMBIOS chassis type -> a word.
///
/// `/sys/class/dmi/id/chassis_type` is a NUMBER from the SMBIOS spec, and the
/// panel was showing it raw: a laptop reported "10". eDEX reads the same file
/// and prints "Notebook", which is the only reason its hardware row reads as
/// information rather than as a serial number.
///
/// Only the types a desktop Linux plausibly runs on are named; anything else
/// keeps its number, which is still more honest than guessing.
fn chassis_name(raw: &str) -> String {
    match raw.trim() {
        "3" => "Desktop",
        "4" => "Low Profile",
        "6" => "Mini Tower",
        "7" => "Tower",
        "8" => "Portable",
        "9" => "Laptop",
        "10" => "Notebook",
        "11" => "Hand Held",
        "13" => "All In One",
        "14" => "Sub Notebook",
        "15" => "Space-saving",
        "23" => "Rack Mount",
        "30" => "Tablet",
        "31" => "Convertible",
        "32" => "Detachable",
        "34" => "Embedded",
        "35" => "Mini PC",
        "36" => "Stick PC",
        // 1 "Other" and 2 "Unknown" are what a VM reports, and both are more
        // useful spelled out than left as a digit.
        "1" => "Other",
        "2" => "Unknown",
        other => return other.to_string(),
    }
    .to_string()
}

fn read_trim(p: &str) -> String {
    fs::read_to_string(p).map(|s| s.trim().to_string()).unwrap_or_default()
}

impl Metrics {
    pub fn new() -> Self {
        let mut sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        sys.refresh_all();
        let cores = sys.cpus().len();
        let hardware = Hardware {
            // DMI is Linux-specific and sysinfo does not expose it. eDEX shows
            // MANUFACTURER / MODEL / CHASSIS, so we read it directly.
            vendor: read_trim("/sys/class/dmi/id/sys_vendor"),
            model: read_trim("/sys/class/dmi/id/product_name"),
            chassis: chassis_name(&read_trim("/sys/class/dmi/id/chassis_type")),
            cpu_model: sys
                .cpus()
                .first()
                .map(|c| clean_cpu(c.brand()))
                .unwrap_or_default(),
            cores,
            host: System::host_name().unwrap_or_default(),
        };
        Self {
            sys,
            hardware,
            per_core: vec![VecDeque::with_capacity(HISTORY); cores],
            memory: Memory::default(),
            procs: Vec::new(),
            temp_c: None,
            tasks: 0,
            uptime: Duration::ZERO,
        }
    }

    pub fn refresh(&mut self) {
        self.refresh_with(true)
    }

    /// `procs = false` skips the process walk.
    ///
    /// Walking /proc is most of what a refresh costs — every pid, every tick —
    /// and the process list changes far more slowly than the CPU graph it
    /// shares a panel with. The shell asks for CPU and memory every second and
    /// for processes every other second; `khadi-bar`, which shows no process
    /// list at all, could stop asking entirely.
    pub fn refresh_with(&mut self, procs: bool) {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        if procs {
            // ONLY CPU AND MEMORY. The default refresh also reads each
            // process's command line, environment, user, root and disk
            // counters — six more files per pid, none of which this draws.
            self.sys.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                true,
                sysinfo::ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );
        }

        for (i, cpu) in self.sys.cpus().iter().enumerate() {
            if let Some(h) = self.per_core.get_mut(i) {
                if h.len() == HISTORY {
                    h.pop_front();
                }
                h.push_back(cpu.cpu_usage());
            }
        }

        self.memory = Memory {
            total: self.sys.total_memory(),
            used: self.sys.used_memory(),
            swap_total: self.sys.total_swap(),
            swap_used: self.sys.used_swap(),
        };

        // Before the guard: these are cheap, and they are read every second
        // whether or not the process list is. Uptime especially — it is a
        // ticking number and freezing it on alternate frames looks broken.
        self.temp_c = Self::hottest_zone();
        self.uptime = Duration::from_secs(System::uptime());

        if !procs {
            return;
        }
        let mut ps: Vec<Process> = self
            .sys
            .processes()
            .values()
            .map(|p| Process {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().to_string(),
                cpu: p.cpu_usage(),
                mem: p.memory(),
            })
            .collect();
        ps.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
        self.tasks = ps.len();
        // Enough to fill a tall side column rather than a fixed sixteen. The
        // panel stops drawing when it runs out of rows, so the cost of extra
        // entries is a short sort, and the benefit is that the band reaches the
        // bottom of the screen instead of trailing off into empty space —
        // which is most of what makes eDEX's panels look finished.
        ps.truncate(40);
        self.procs = ps;
    }

    /// Highest thermal zone. sysinfo's component API is inconsistent across
    /// platforms and often empty in a VM; /sys/class/thermal is not.
    fn hottest_zone() -> Option<f32> {
        let mut hottest: Option<f32> = None;
        let dir = fs::read_dir("/sys/class/thermal").ok()?;
        for e in dir.flatten() {
            let p = e.path().join("temp");
            if let Ok(s) = fs::read_to_string(&p) {
                if let Ok(milli) = s.trim().parse::<f32>() {
                    let c = milli / 1000.0;
                    if c > 0.0 && c < 150.0 {
                        hottest = Some(hottest.map_or(c, |h: f32| h.max(c)));
                    }
                }
            }
        }
        hottest
    }

    /// Mean across all cores, latest sample.
    pub fn cpu_avg(&self) -> f32 {
        let last: Vec<f32> = self.per_core.iter().filter_map(|h| h.back().copied()).collect();
        if last.is_empty() { 0.0 } else { last.iter().sum::<f32>() / last.len() as f32 }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded() {
        // An unbounded deque is a slow leak in a process meant to run for days.
        let mut m = Metrics::new();
        for _ in 0..(HISTORY + 20) {
            m.refresh();
        }
        for h in &m.per_core {
            assert!(h.len() <= HISTORY, "history grew past {HISTORY}");
        }
    }

    #[test]
    fn cpu_brand_is_trimmed_to_fit() {
        assert_eq!(
            clean_cpu("Intel(R) Core(TM) i7-7500U CPU @ 2.70GHz"),
            "Intel Core i7-7500U"
        );
        assert_eq!(clean_cpu("AMD Ryzen 7 5800X 8-Core Processor"), "AMD Ryzen 7 5800X 8-Core");
    }

    #[test]
    fn reports_real_cores() {
        let m = Metrics::new();
        assert!(m.hardware.cores >= 1);
        assert_eq!(m.per_core.len(), m.hardware.cores);
    }
}
