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
    pub gateway: String,
    pub dns: String,
    pub mac: String,
    pub mtu: String,
    pub up: bool,
}

/// Established and listening TCP, from /proc/net/tcp{,6}.
///
/// eDEX's right column listed live connection endpoints and resolved each one
/// through MaxMind. The geolocation is gone for good — GeoLite2 needs an
/// account and carries redistribution terms an ISO should not take on — but
/// the *endpoints* were never the part that needed a database, and without
/// them the network column ran out of content a third of the way down.
#[derive(Debug, Clone, Default)]
pub struct Sockets {
    pub established: usize,
    pub listening: usize,
    /// Remote `addr:port` of established connections, most-repeated first.
    pub peers: Vec<(String, usize)>,
    /// Local `addr:port` this host accepts on, lowest port first.
    pub listeners: Vec<String>,
}

/// `/proc/net/tcp` writes addresses as native-endian hex words, which on
/// every machine this runs on means the IPv4 octets arrive backwards.
fn parse_v4(hex: &str) -> Option<String> {
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(format!("{}.{}.{}.{}", n & 0xff, (n >> 8) & 0xff, (n >> 16) & 0xff, n >> 24))
}

/// IPv6 is four of those words. Printed compressed, because a 34-column panel
/// has no room for the expanded form and an elided address still identifies
/// the peer well enough to recognise it.
fn parse_v6(hex: &str) -> Option<String> {
    if hex.len() != 32 {
        return None;
    }
    let mut seg = Vec::with_capacity(8);
    for w in 0..4 {
        let n = u32::from_str_radix(&hex[w * 8..w * 8 + 8], 16).ok()?;
        seg.push(((n & 0xff) << 8) | ((n >> 8) & 0xff));
        seg.push((((n >> 16) & 0xff) << 8) | ((n >> 24) & 0xff));
    }
    // Collapse the longest run of zero groups, as RFC 5952 prints it. A
    // textual search-and-replace for ":0:0:" gets `::1` wrong — it leaves the
    // leading group behind — so the run is found on the numbers.
    let (mut best, mut best_len) = (0usize, 0usize);
    let mut i = 0;
    while i < seg.len() {
        if seg[i] != 0 {
            i += 1;
            continue;
        }
        let j = seg[i..].iter().take_while(|&&x| x == 0).count();
        if j > best_len {
            best = i;
            best_len = j;
        }
        i += j;
    }
    let hex = |r: &[u32]| r.iter().map(|x| format!("{x:x}")).collect::<Vec<_>>().join(":");
    if best_len < 2 {
        return Some(hex(&seg));
    }
    Some(format!("{}::{}", hex(&seg[..best]), hex(&seg[best + best_len..])))
}

impl Sockets {
    pub fn read() -> Self {
        let mut out = Self::default();
        let mut tally: Vec<(String, usize)> = Vec::new();
        let mut listen: Vec<(u16, String)> = Vec::new();
        for (path, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
            let txt = match fs::read_to_string(path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            for line in txt.lines().skip(1) {
                let f: Vec<&str> = line.split_whitespace().collect();
                if f.len() < 4 {
                    continue;
                }
                let field = match f[3] {
                    // A listening socket is identified by where it binds, not
                    // by its (empty) remote address.
                    "0A" => {
                        out.listening += 1;
                        f[1]
                    }
                    "01" => {
                        out.established += 1;
                        f[2]
                    }
                    _ => continue,
                };
                let (addr, port) = match field.split_once(':') {
                    Some(p) => p,
                    None => continue,
                };
                let ip = if v6 { parse_v6(addr) } else { parse_v4(addr) };
                let ip = match ip {
                    Some(i) => i,
                    None => continue,
                };
                let port = u16::from_str_radix(port, 16).unwrap_or(0);
                if f[3] == "0A" {
                    // A wildcard bind prints as the bare port, which is how
                    // `ss -l` shows it and how anyone reads it.
                    let key = if ip == "0.0.0.0" || ip == "::" {
                        format!("*:{port}")
                    } else {
                        format!("{ip}:{port}")
                    };
                    if !listen.iter().any(|(_, k)| *k == key) {
                        listen.push((port, key));
                    }
                    continue;
                }
                let key = format!("{ip}:{port}");
                match tally.iter_mut().find(|(k, _)| *k == key) {
                    Some((_, n)) => *n += 1,
                    None => tally.push((key, 1)),
                }
            }
        }
        tally.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out.peers = tally;
        listen.sort();
        out.listeners = listen.into_iter().map(|(_, k)| k).collect();
        out
    }
}

pub struct Network {
    pub iface: Iface,
    pub sockets: Sockets,
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
            sockets: Sockets::default(),
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
    fn default_iface() -> Option<(String, String)> {
        let txt = fs::read_to_string("/proc/net/route").ok()?;
        for line in txt.lines().skip(1) {
            let f: Vec<&str> = line.split_whitespace().collect();
            // destination 00000000 is the default route; field 2 is its gateway
            if f.len() > 2 && f[1] == "00000000" {
                return Some((f[0].to_string(), parse_v4(f[2]).unwrap_or_default()));
            }
        }
        None
    }

    /// First nameserver in resolv.conf. systemd-resolved writes a stub at
    /// 127.0.0.53 and that is the honest answer for where queries go, so it is
    /// not filtered out.
    fn first_nameserver() -> String {
        fs::read_to_string("/etc/resolv.conf")
            .ok()
            .and_then(|t| {
                t.lines()
                    .find_map(|l| l.strip_prefix("nameserver ").map(|a| a.trim().to_string()))
            })
            .unwrap_or_else(|| "\u{2014}".into())
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
        let (name, gateway) = Self::default_iface().unwrap_or_default();
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

        self.sockets = Sockets::read();

        let sys = |k: &str| {
            fs::read_to_string(format!("/sys/class/net/{name}/{k}"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        self.iface = Iface {
            up: !name.is_empty(),
            ipv4: Self::outbound_ipv4().unwrap_or_else(|| "\u{2014}".into()),
            gateway: if gateway.is_empty() { "\u{2014}".into() } else { gateway },
            dns: Self::first_nameserver(),
            mac: sys("address"),
            mtu: sys("mtu"),
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
    fn hex_addresses_come_back_the_right_way_round() {
        // 0100007F is 127.0.0.1 written native-endian: the octets arrive
        // backwards and reading them forwards gives 1.0.0.127, which looks
        // like a plausible address and is why this has a test.
        assert_eq!(parse_v4("0100007F").as_deref(), Some("127.0.0.1"));
        assert_eq!(parse_v4("3A64A8C0").as_deref(), Some("192.168.100.58"));
        assert_eq!(parse_v4("zz").as_deref(), None);
        assert_eq!(parse_v6("00000000000000000000000001000000").as_deref(), Some("::1"));
        assert_eq!(parse_v6("short").as_deref(), None);
    }

    #[test]
    fn sockets_read_without_panicking() {
        // Reads the real /proc; the shape of the answer is what matters.
        let s = Sockets::read();
        assert!(s.peers.len() <= s.established.max(1) * 64);
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
