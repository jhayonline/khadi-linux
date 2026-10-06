//! Network metrics.
//!
//! Deliberately no geolocation. eDEX resolved connection endpoints with
//! MaxMind's database (`netstat.class.js`: "Prevent geoip lookup attempt until
//! maxminddb is loaded") and plotted them on a WebGL globe. GeoLite2 now needs
//! an account and carries redistribution terms, and a terminal cannot render
//! WebGL at all — so Khadi shows a world map without claiming to know where
//! anyone is.

use std::{collections::VecDeque, fs, net::UdpSocket, time::Instant};

const HISTORY: usize = 60;

#[derive(Debug, Clone, Default)]
pub struct Iface {
    pub name: String,
    pub ipv4: String,
    pub up: bool,
}

pub struct Network {
    pub iface: Iface,
    pub rx_rate: f64, // bytes/sec
    pub tx_rate: f64,
    pub rx_total: u64,
    pub tx_total: u64,
    pub rx_hist: VecDeque<f64>,
    pub tx_hist: VecDeque<f64>,
    last: Option<(u64, u64, Instant)>,
}

impl Network {
    pub fn new() -> Self {
        Self {
            iface: Iface::default(),
            rx_rate: 0.0,
            tx_rate: 0.0,
            rx_total: 0,
            tx_total: 0,
            rx_hist: VecDeque::with_capacity(HISTORY),
            tx_hist: VecDeque::with_capacity(HISTORY),
            last: None,
        }
    }

    /// Default-route interface, from /proc/net/route. sysinfo enumerates
    /// interfaces but will not say which one carries traffic to the world.
    fn default_iface() -> Option<String> {
        let txt = fs::read_to_string("/proc/net/route").ok()?;
        for line in txt.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            // destination 00000000 is the default route
            if f.len() > 1 && f[1] == "00000000" {
                return Some(f[0].to_string());
            }
        }
        None
    }

    /// Outbound address, without sending anything. A connected UDP socket
    /// only sets a destination for later writes — no packet leaves the host —
    /// and the kernel then reports which local address routing chose. Reading
    /// /proc/net/fib_trie would mean parsing a tree dump for the same answer.
    fn outbound_ipv4() -> Option<String> {
        let s = UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect("192.0.2.1:9").ok()?; // TEST-NET-1, RFC 5737: never routed
        Some(s.local_addr().ok()?.ip().to_string())
    }

    pub fn refresh(&mut self) {
        let name = Self::default_iface().unwrap_or_default();
        let (mut rx, mut tx) = (0u64, 0u64);
        if let Ok(txt) = fs::read_to_string("/proc/net/dev") {
            for line in txt.lines().skip(2) {
                let (dev, rest) = match line.split_once(':') {
                    Some(p) => p,
                    None => continue,
                };
                if dev.trim() != name {
                    continue;
                }
                let f: Vec<&str> = rest.split_whitespace().collect();
                if f.len() >= 9 {
                    rx = f[0].parse().unwrap_or(0);
                    tx = f[8].parse().unwrap_or(0);
                }
            }
        }
        let now = Instant::now();
        if let Some((prx, ptx, pt)) = self.last {
            let dt = now.duration_since(pt).as_secs_f64().max(0.001);
            // Counters wrap and interfaces reset; saturating_sub keeps a reset
            // from reading as a terabyte spike.
            self.rx_rate = rx.saturating_sub(prx) as f64 / dt;
            self.tx_rate = tx.saturating_sub(ptx) as f64 / dt;
        }
        self.last = Some((rx, tx, now));
        self.rx_total = rx;
        self.tx_total = tx;

        for (h, v) in [(&mut self.rx_hist, self.rx_rate), (&mut self.tx_hist, self.tx_rate)] {
            if h.len() == HISTORY {
                h.pop_front();
            }
            h.push_back(v);
        }

        self.iface = Iface {
            up: !name.is_empty(),
            ipv4: Self::outbound_ipv4().unwrap_or_else(|| "—".into()),
            name,
        };
    }
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

/// Human-readable rate. eDEX labels its traffic panel "UP / DOWN, MB/S" but a
/// fixed unit reads as 0.00 for most of a session.
pub fn rate(bytes_per_sec: f64) -> String {
    const K: f64 = 1024.0;
    if bytes_per_sec < K {
        format!("{bytes_per_sec:.0} B/s")
    } else if bytes_per_sec < K * K {
        format!("{:.1} K/s", bytes_per_sec / K)
    } else {
        format!("{:.1} M/s", bytes_per_sec / (K * K))
    }
}

/// Human-readable total.
pub fn total(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K * K {
        format!("{:.0} KiB", b / K)
    } else if b < K * K * K {
        format!("{:.1} MiB", b / (K * K))
    } else {
        format!("{:.2} GiB", b / (K * K * K))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_pick_a_sensible_unit() {
        assert_eq!(rate(512.0), "512 B/s");
        assert_eq!(rate(2048.0), "2.0 K/s");
        assert_eq!(rate(5.0 * 1024.0 * 1024.0), "5.0 M/s");
    }

    #[test]
    fn history_is_bounded() {
        let mut n = Network::new();
        for _ in 0..(HISTORY + 20) {
            n.refresh();
        }
        assert!(n.rx_hist.len() <= HISTORY);
        assert!(n.tx_hist.len() <= HISTORY);
    }

    #[test]
    fn a_counter_reset_is_not_a_spike() {
        // Interfaces come and go and counters wrap. Treating that as traffic
        // puts a terabyte-per-second spike on the graph.
        let mut n = Network::new();
        n.last = Some((1_000_000, 1_000_000, Instant::now()));
        n.refresh();
        assert!(n.rx_rate >= 0.0 && n.rx_rate.is_finite());
    }
}
