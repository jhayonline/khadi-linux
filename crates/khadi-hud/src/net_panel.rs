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
use crate::widgets::{pair, Header, Spark};

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

        room!(2);
        let half = w / 2;
        pair(buf, Rect::new(area.x, y, half, 2), "STATE",
             if self.n.iface.up { "ONLINE" } else { "OFFLINE" }, t);
        pair(buf, Rect::new(area.x + half, y, w - half, 2), "IFACE", iface, t);
        y += 3;

        room!(2);
        pair(buf, Rect::new(area.x, y, w, 2), "IPv4", &self.n.iface.ipv4, t);
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

        room!(Header::HEIGHT);
        Header::new("NETWORK TRAFFIC", "UP / DOWN", t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;

        room!(2);
        let half = w / 2;
        pair(buf, Rect::new(area.x, y, half, 2), "DOWN", &rate(self.n.rx_rate), t);
        pair(buf, Rect::new(area.x + half, y, w - half, 2), "UP", &rate(self.n.tx_rate), t);
        y += 2;

        // Sparklines are scaled to the window's own peak: absolute byte rates
        // span six orders of magnitude and a fixed scale is flat or clipped.
        room!(2);
        let peak = self
            .n
            .rx_hist
            .iter()
            .chain(self.n.tx_hist.iter())
            .cloned()
            .fold(1.0f64, f64::max);
        let norm = |h: &std::collections::VecDeque<f64>| -> Vec<f32> {
            h.iter().map(|v| (v / peak * 100.0) as f32).collect()
        };
        let rx = norm(&self.n.rx_hist);
        let tx = norm(&self.n.tx_hist);
        Spark { data: &rx, theme: t }.render(Rect::new(area.x, y, w, 1), buf);
        Spark { data: &tx, theme: t }.render(Rect::new(area.x, y + 1, w, 1), buf);
        y += 3;

        room!(2);
        pair(buf, Rect::new(area.x, y, half, 2), "TOTAL IN", &total(self.n.rx_total), t);
        pair(buf, Rect::new(area.x + half, y, w - half, 2), "TOTAL OUT",
             &total(self.n.tx_total), t);
        let _ = buf;
        let _ = Style::default();
    }
}
