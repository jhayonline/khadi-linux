//! The network column.
//!
//! eDEX's right column is NETWORK STATUS, WORLD VIEW and NETWORK TRAFFIC.
//!
//! The world view is the interesting one. eDEX drew a WebGL globe
//! ([encom-globe](https://github.com/arscan/encom-globe)) and pinned
//! connection endpoints resolved through MaxMind. The globe is in `globe.rs`
//! — a cell grid cannot render WebGL, but it can turn a dotted sphere, which
//! is the part that actually reads. The geolocation stays gone: GeoLite2
//! needs an account and carries redistribution terms an ISO should not take
//! on, so the panel shows the world without claiming to know where anyone is.

use khadi_core::{net::{rate, total, Network}, Theme};
use ratatui::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};

use crate::globe::Globe;
use crate::widgets::{col, ellipsize, grid, pair, Graph, Header};

pub struct NetPanel<'a> {
    pub n: &'a Network,
    pub theme: &'a Theme,
    /// Globe rotation in degrees. The panel does not own a clock — `--once`
    /// has to be able to render the same frame twice.
    pub spin: f64,
}

impl Widget for NetPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let t = self.theme;
        let w = area.width;
        let mut y = area.y;
        let bottom = area.y + area.height;
        macro_rules! room {
            ($n:expr) => {
                if y + $n > bottom {
                    return;
                }
            };
        }

        room!(Header::HEIGHT);
        let iface = if self.n.iface.name.is_empty() { "—" } else { &self.n.iface.name };
        Header::new("PANEL", "NETWORK", t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;

        // One band, three cells. This was three two-row pairs with a blank
        // row between each — nine rows to carry three short strings, in the
        // column that then ran out of content a third of the way down.
        room!(2);
        grid(buf, Rect::new(area.x, y, w, 2), &[
            ("STATE", if self.n.iface.up { "ONLINE" } else { "OFFLINE" }),
            ("IFACE", iface),
            ("IPv4", &self.n.iface.ipv4),
        ], t);
        y += 3;

        // The rest of what a network column is for. eDEX's NETWORK STATUS
        // block carried this and Khadi's did not, which is why the column ran
        // out of content: three readouts cannot fill a full-height panel, and
        // on a machine with nothing dialling out the sockets band is empty too.
        room!(2);
        grid(buf, Rect::new(area.x, y, w, 2), &[
            ("GATEWAY", &self.n.iface.gateway),
            ("DNS", &self.n.iface.dns),
        ], t);
        y += 3;

        room!(2);
        grid(buf, Rect::new(area.x, y, w, 2), &[
            ("MAC", &self.n.iface.mac),
            ("MTU", &self.n.iface.mtu),
        ], t);
        y += 3;

        // WORLD VIEW. No pins: there is no geolocation behind this and a pin
        // would be a claim the data does not support.
        room!(Header::HEIGHT);
        Header::new("WORLD VIEW", "NO GEOIP", t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;
        // The globe wants every row it can get: the disc radius is set by the
        // smaller of the two subpixel axes, so height is what limits it in a
        // 34-column panel.
        let map_h = 14u16.min(bottom.saturating_sub(y));
        if map_h >= 4 {
            Globe { theme: t, spin: self.spin }
                .render(Rect::new(area.x, y, w, map_h), buf);
            y += map_h + 1;
        }

        // The peak belongs in the header slot. It used to be drawn flush
        // right on the graph's own top row, where the trace runs through it
        // the moment traffic reaches the top of the window.
        room!(Header::HEIGHT);
        let peak = self
            .n
            .rx_hist
            .iter()
            .chain(self.n.tx_hist.iter())
            .cloned()
            .fold(1.0f64, f64::max);
        Header::new("NETWORK TRAFFIC", &format!("PEAK {}", rate(peak)), t)
            .render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;

        room!(2);
        let half = w / 2;
        pair(buf, Rect::new(area.x, y, half, 2), "DOWN", &rate(self.n.rx_rate), t);
        pair(buf, Rect::new(area.x + half, y, w - half, 2), "UP", &rate(self.n.tx_rate), t);
        y += 2;

        // Both directions in ONE graph, as eDEX has it, scaled to the window's
        // own peak: absolute byte rates span six orders of magnitude and a
        // fixed scale is either flat or clipped.
        room!(4);
        let norm = |h: &std::collections::VecDeque<f64>| -> Vec<f32> {
            h.iter().map(|v| (v / peak * 100.0) as f32).collect()
        };
        let rx = norm(&self.n.rx_hist);
        let tx = norm(&self.n.tx_hist);
        let series: Vec<&[f32]> = vec![&rx, &tx];
        Graph { series: &series, max: 100.0, theme: t }
            .render(Rect::new(area.x, y, w, 3), buf);
        y += 4;

        room!(2);
        pair(buf, Rect::new(area.x, y, half, 2), "TOTAL IN", &total(self.n.rx_total), t);
        pair(buf, Rect::new(area.x + half, y, w - half, 2), "TOTAL OUT",
             &total(self.n.tx_total), t);
        y += 3;

        // SOCKETS. eDEX listed live endpoints here; so does this, minus the
        // geolocation. It is also what fills the bottom of the column — the
        // panel used to stop at TOTAL OUT and leave a third of its height
        // empty, which is the single most visible difference against the
        // reference screenshot.
        room!(Header::HEIGHT);
        let s = &self.n.sockets;
        Header::new("SOCKETS", &format!("{} ESTABLISHED", s.established), t)
            .render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;

        if s.peers.is_empty() && y < bottom {
            buf.set_string(area.x, y, "none", Style::default().fg(col(t.text_muted)));
            y += 1;
        }
        for (addr, n) in &s.peers {
            if y >= bottom {
                return;
            }
            // The repeat count earns its column only when a peer holds more
            // than one socket, which is what a browser or a sync client looks
            // like and a one-off request does not.
            let tag = if *n > 1 { format!("x{n}") } else { String::new() };
            let room = w.saturating_sub(tag.chars().count() as u16 + 1) as usize;
            // Elide from the LEFT: the port and the low octets are what tell
            // two connections to the same host apart.
            let shown: String = if addr.chars().count() > room {
                let skip = addr.chars().count() - room + 1;
                format!("\u{2026}{}", addr.chars().skip(skip).collect::<String>())
            } else {
                addr.clone()
            };
            buf.set_string(area.x, y, &shown, Style::default().fg(col(t.text)));
            if !tag.is_empty() {
                buf.set_string(area.x + w - tag.chars().count() as u16, y, &tag,
                               Style::default().fg(col(t.text_muted)));
            }
            y += 1;
        }
        y += 1;

        // What this host ACCEPTS, as its own band. A machine with nothing
        // dialling out still has services listening, so this is the band that
        // keeps the bottom of the column from being empty — which on a freshly
        // booted VM is every time.
        room!(Header::HEIGHT);
        Header::new("LISTENING", &format!("{} PORTS", s.listening), t)
            .render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;
        for addr in &s.listeners {
            if y >= bottom {
                return;
            }
            buf.set_string(area.x, y, ellipsize(addr, w as usize),
                           Style::default().fg(col(t.text_dim)));
            y += 1;
        }
    }
}
