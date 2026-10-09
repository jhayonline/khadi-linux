//! Places for the globe: where this machine is, and where the hosts it is connected to
//! are.
//!
//! Positions come from looking addresses up at an online service, [`SERVICE`]. That
//! tells the service which public addresses this machine talks to, so it can be turned
//! off, each address is asked about only once, and this machine's own position can be
//! set by hand instead (see `config`).

use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The lookup service: no key, HTTPS, several addresses per request.
const SERVICE: &str = "https://get.geojs.io/v1/ip/geo.json";
/// How many addresses to ask about per round, and how many places to show at once.
const BATCH: usize = 20;
const MAX_LINKS: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub lat: f32,
    pub lon: f32,
    pub label: String,
}

/// What the globe shows at one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub home: Option<Place>,
    /// Whether `home` was set by the user rather than looked up.
    pub home_is_set: bool,
    /// Where the hosts with open connections are, without repeats.
    pub links: Vec<Place>,
}

/// Reads `latitude, longitude[, name]`, as given on the command line or in the
/// configuration.
pub fn parse_location(text: &str) -> Option<Place> {
    let mut parts = text.splitn(3, ',').map(str::trim);
    let lat: f32 = parts.next()?.parse().ok()?;
    let lon: f32 = parts.next()?.parse().ok()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    let label = parts.next().filter(|name| !name.is_empty()).unwrap_or("HERE");
    Some(Place {
        lat,
        lon,
        label: label.to_string(),
    })
}

/// Whether an address is one a lookup service could place: not private, local or
/// otherwise special.
pub fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || a == 0
                // Carrier-grade NAT and the benchmarking range.
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (18..20).contains(&b)))
        }
        // Global unicast only.
        IpAddr::V6(v6) => v6.segments()[0] & 0xe000 == 0x2000,
    }
}

/// The remote ends of established connections, from the kernel's `/proc/net/tcp` or
/// `/proc/net/tcp6` table.
pub fn parse_connections(table: &str) -> Vec<IpAddr> {
    let mut remotes = Vec::new();
    for line in table.lines().skip(1) {
        let mut fields = line.split_whitespace();
        let (Some(remote), Some(state)) = (fields.nth(2), fields.next()) else {
            continue;
        };
        // 01 is an established connection.
        if state != "01" {
            continue;
        }
        let Some((address, _port)) = remote.split_once(':') else {
            continue;
        };
        // Each 32-bit word is written in the machine's byte order: reversed, here.
        let words: Option<Vec<[u8; 4]>> = address
            .as_bytes()
            .chunks(8)
            .map(|chunk| {
                let word = u32::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
                Some(word.to_le_bytes())
            })
            .collect();
        let ip = match words.as_deref() {
            Some([v4]) => IpAddr::V4(Ipv4Addr::from(*v4)),
            Some([a, b, c, d]) => {
                let mut bytes = [0u8; 16];
                for (i, word) in [a, b, c, d].into_iter().enumerate() {
                    bytes[i * 4..i * 4 + 4].copy_from_slice(word);
                }
                let v6 = Ipv6Addr::from(bytes);
                // IPv4 connections on a dual-stack socket show up in the IPv6 table.
                v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4)
            }
            _ => continue,
        };
        if !remotes.contains(&ip) {
            remotes.push(ip);
        }
    }
    remotes
}

/// Reads the service's answer: one object, or a list of them. Entries it could not
/// place are left out.
pub fn parse_answer(json: &str) -> Vec<(String, Place)> {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let entries = match value {
        Value::Array(entries) => entries,
        entry => vec![entry],
    };
    entries
        .iter()
        .filter_map(|entry| {
            // Coordinates arrive as text, and as "nil" when unknown.
            let number = |key: &str| -> Option<f32> {
                match entry.get(key)? {
                    Value::String(text) => text.parse().ok(),
                    Value::Number(number) => number.as_f64().map(|n| n as f32),
                    _ => None,
                }
            };
            let text = |key: &str| entry.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
            let label = match (text("city"), text("country_code"), text("country")) {
                (Some(city), Some(code), _) => format!("{city}, {code}"),
                (Some(city), None, _) => city.to_string(),
                (None, _, Some(country)) => country.to_string(),
                (None, Some(code), None) => code.to_string(),
                (None, None, None) => "UNKNOWN".to_string(),
            };
            Some((
                text("ip")?.to_string(),
                Place {
                    lat: number("latitude")?,
                    lon: number("longitude")?,
                    label,
                },
            ))
        })
        .collect()
}

/// Asks the service about `ips`, or about this machine's own address when there are
/// none. Runs `curl`, so that no network code has to live in this program.
fn ask(ips: &[IpAddr]) -> Vec<(String, Place)> {
    let mut url = SERVICE.to_string();
    if !ips.is_empty() {
        let list: Vec<String> = ips.iter().map(IpAddr::to_string).collect();
        url.push_str(&format!("?ip={}", list.join(",")));
    }
    let output = std::process::Command::new("curl")
        .args(["-fsS", "-m", "8", "--proto", "=https", &url])
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(output) if output.status.success() => parse_answer(&String::from_utf8_lossy(&output.stdout)),
        _ => Vec::new(),
    }
}

fn current_connections() -> Vec<IpAddr> {
    let mut remotes = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(text) = std::fs::read_to_string(table) {
            for ip in parse_connections(&text) {
                if is_public(&ip) && !remotes.contains(&ip) {
                    remotes.push(ip);
                }
            }
        }
    }
    remotes
}

/// Places known from earlier runs, kept in the cache directory so that an address is
/// asked about once, not once per session.
struct Cache {
    path: Option<PathBuf>,
    /// `None` records an address the service could not place.
    known: HashMap<IpAddr, Option<Place>>,
}

impl Cache {
    fn load() -> Cache {
        let path = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
            .map(|base| base.join("edex-rs/places.tsv"));
        let mut known = HashMap::new();
        if let Some(text) = path.as_ref().and_then(|path| std::fs::read_to_string(path).ok()) {
            for line in text.lines() {
                let mut fields = line.splitn(4, '\t');
                let parsed = (|| {
                    let ip: IpAddr = fields.next()?.parse().ok()?;
                    let lat = fields.next()?.parse().ok()?;
                    let lon = fields.next()?.parse().ok()?;
                    let label = fields.next()?.to_string();
                    Some((ip, Place { lat, lon, label }))
                })();
                if let Some((ip, place)) = parsed {
                    known.insert(ip, Some(place));
                }
            }
        }
        Cache { path, known }
    }

    fn remember(&mut self, ip: IpAddr, place: Option<Place>) {
        if let (Some(place), Some(path)) = (&place, &self.path) {
            let _ = path.parent().map(std::fs::create_dir_all);
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let label = place.label.replace(['\t', '\n'], " ");
                let _ = writeln!(file, "{ip}\t{}\t{}\t{label}", place.lat, place.lon);
            }
        }
        self.known.insert(ip, place);
    }
}

/// The places of `remotes`, nearby ones merged so that a data centre is one mark.
fn links_for(remotes: &[IpAddr], cache: &Cache) -> Vec<Place> {
    let mut links: Vec<Place> = Vec::new();
    for place in remotes.iter().filter_map(|ip| cache.known.get(ip)?.as_ref()) {
        let near = |other: &Place| (other.lat - place.lat).abs() < 1.0 && (other.lon - place.lon).abs() < 1.0;
        if !links.iter().any(near) {
            links.push(place.clone());
        }
        if links.len() == MAX_LINKS {
            break;
        }
    }
    links
}

/// Keeps a [`Snapshot`] up to date from a background thread.
pub struct Geo {
    shared: Arc<Mutex<Snapshot>>,
}

impl Geo {
    /// `lookups` allows asking the service; `location` is this machine's position when
    /// the user has set it.
    pub fn start(lookups: bool, location: Option<Place>) -> Geo {
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let writer = shared.clone();
        std::thread::spawn(move || {
            let home_is_set = location.is_some();
            if let Ok(mut snapshot) = writer.lock() {
                snapshot.home = location;
                snapshot.home_is_set = home_is_set;
            }
            if !lookups {
                return;
            }
            let mut cache = Cache::load();
            let mut home_asked: Option<Instant> = None;
            // Stops once the shell that reads this is gone.
            while Arc::strong_count(&writer) > 1 {
                // This machine's own position, again now and then: laptops travel.
                let due = home_asked.is_none_or(|at| at.elapsed() > Duration::from_secs(1800));
                if !home_is_set && due {
                    home_asked = Some(Instant::now());
                    if let Some((_, place)) = ask(&[]).into_iter().next() {
                        if let Ok(mut snapshot) = writer.lock() {
                            snapshot.home = Some(place);
                        }
                    }
                }

                let remotes = current_connections();
                let unknown: Vec<IpAddr> = remotes
                    .iter()
                    .filter(|ip| !cache.known.contains_key(ip))
                    .take(BATCH)
                    .copied()
                    .collect();
                if !unknown.is_empty() {
                    let answers = ask(&unknown);
                    for ip in unknown {
                        let place = answers
                            .iter()
                            .find(|(answered, _)| answered.parse() == Ok(ip))
                            .map(|(_, place)| place.clone());
                        cache.remember(ip, place);
                    }
                }
                if let Ok(mut snapshot) = writer.lock() {
                    snapshot.links = links_for(&remotes, &cache);
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        });
        Geo { shared }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.lock().map(|snapshot| snapshot.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_locations() {
        let place = parse_location("48.85, 2.35, Paris").unwrap();
        assert_eq!((place.lat, place.lon, place.label.as_str()), (48.85, 2.35, "Paris"));
        assert_eq!(parse_location("51.5,-0.12").unwrap().label, "HERE");
        assert_eq!(parse_location("1,2,Name, with comma").unwrap().label, "Name, with comma");
        for bad in ["", "north", "91,0", "0,181", "12"] {
            assert_eq!(parse_location(bad), None, "{bad}");
        }
    }

    #[test]
    fn tells_public_addresses_apart() {
        for public in ["8.8.8.8", "142.250.1.1", "2606:4700::1111"] {
            assert!(is_public(&public.parse().unwrap()), "{public}");
        }
        for special in ["127.0.0.1", "10.1.2.3", "192.168.1.20", "172.16.0.1", "169.254.1.1", "100.64.0.1", "0.0.0.0", "224.0.0.1", "::1", "fe80::1", "fd00::1"] {
            assert!(!is_public(&special.parse().unwrap()), "{special}");
        }
    }

    #[test]
    fn reads_the_connection_tables() {
        let v4 = "  sl  local_address rem_address   st tx_queue\n\
                  0: 3964A8C0:D431 08080808:01BB 01 00000000:00000000\n\
                  1: 00000000:0016 00000000:0000 0A 00000000:00000000\n\
                  2: 3964A8C0:D432 08080808:01BB 01 00000000:00000000\n\
                  3: 0100007F:1F90 0100007F:C350 01 00000000:00000000\n";
        let expected: Vec<IpAddr> = vec!["8.8.8.8".parse().unwrap(), "127.0.0.1".parse().unwrap()];
        assert_eq!(parse_connections(v4), expected);

        let v6 = "  sl  local_address remote_address st\n\
                  0: 00000000000000000000000001000000:1F90 0000000000000000FFFF00000101A8C0:C350 01\n\
                  1: 000000000000000000000000000000000:0050 00470626000000000000000011110000:01BB 01\n\
                  2: garbage\n";
        let expected: Vec<IpAddr> = vec!["192.168.1.1".parse().unwrap(), "2606:4700::1111".parse().unwrap()];
        assert_eq!(parse_connections(v6), expected);
    }

    #[test]
    fn reads_the_service_answer() {
        let one = r#"{"ip":"8.8.8.8","latitude":"37.751","longitude":"-97.822","country":"United States","country_code":"US"}"#;
        let places = parse_answer(one);
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].0, "8.8.8.8");
        assert_eq!(places[0].1.label, "United States");
        assert!((places[0].1.lon + 97.822).abs() < 1e-3);

        let many = r#"[{"ip":"1.1.1.1","latitude":"nil","longitude":"nil"},
                       {"ip":"9.9.9.9","latitude":"47.4","longitude":"8.5","city":"Zurich","country_code":"CH"}]"#;
        let places = parse_answer(many);
        assert_eq!(places.len(), 1, "an address without a position is left out");
        assert_eq!(places[0].1.label, "Zurich, CH");
        assert!(parse_answer("not json").is_empty());
    }

    #[test]
    fn nearby_links_are_one_mark() {
        let place = |lat, lon| Some(Place { lat, lon, label: String::new() });
        let ips: Vec<IpAddr> = ["1.0.0.1", "1.0.0.2", "1.0.0.3", "1.0.0.4"].iter().map(|ip| ip.parse().unwrap()).collect();
        let mut cache = Cache { path: None, known: HashMap::new() };
        cache.known.insert(ips[0], place(37.0, -122.0));
        cache.known.insert(ips[1], place(37.4, -122.3));
        cache.known.insert(ips[2], place(51.5, 0.0));
        cache.known.insert(ips[3], None);
        assert_eq!(links_for(&ips, &cache).len(), 2);
    }
}
