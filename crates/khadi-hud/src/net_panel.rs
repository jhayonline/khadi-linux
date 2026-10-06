//! The network column.
//!
//! eDEX's right column is NETWORK STATUS, WORLD VIEW and NETWORK TRAFFIC.
//!
//! The world view is the interesting one. eDEX drew a WebGL globe
//! ([encom-globe](https://github.com/arscan/encom-globe), MIT, 975 KB of JS
//! plus 940 KB of grid data) and pinned connection endpoints resolved through
//! MaxMind. Neither survives into Khadi: a cell grid cannot render WebGL at
//! any price, and GeoLite2 needs an account and carries redistribution terms
//! that an ISO should not take on.
//!
//! What a terminal CAN do is a world map, because ratatui ships one — braille
//! markers over ~5000 points. So the panel keeps eDEX's silhouette without
//! the browser engine and without claiming to know where anyone is.

use khadi_core::{net::{rate, total, Network}, Theme};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    symbols::Marker,
    widgets::{
        canvas::{Canvas, Map, MapResolution},
        Widget,
    },
};

use crate::widgets::{col, pair, Header, Spark};

pub struct NetPanel<'a> {
    pub n: &'a Network,
    pub theme: &'a Theme,
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
        let map_h = 9u16.min(bottom.saturating_sub(y));
        if map_h >= 4 {
            let line = col(t.a(50));
            Canvas::default()
                .marker(Marker::Braille)
                .x_bounds([-180.0, 180.0])
                .y_bounds([-90.0, 90.0])
                .paint(|ctx| {
                    ctx.draw(&Map { resolution: MapResolution::High, color: line });
                })
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
