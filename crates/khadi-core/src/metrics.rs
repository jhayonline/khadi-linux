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
            chassis: read_trim("/sys/class/dmi/id/chassis_type"),
            cpu_model: sys
                .cpus()
                .first()
                .map(|c| clean_cpu(c.brand()))
                .unwrap_or_default(),
            cores,
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
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

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
        ps.truncate(16);
        self.procs = ps;

        self.temp_c = Self::hottest_zone();
        self.uptime = Duration::from_secs(System::uptime());
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
