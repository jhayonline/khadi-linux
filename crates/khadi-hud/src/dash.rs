//! The dashboard.
//!
//! Built for a 34-column side panel, because that is eDEX's actual proportion
//! and the measured reason btop cannot occupy one: its CPU box needs 60
//! columns, its process box 44. The layout degrades by dropping panels from
//! the bottom rather than by refusing to draw, so a narrow or short terminal
//! still shows something true.

use khadi_core::{metrics::Metrics, Theme};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::Widget,
};

use crate::widgets::{col, pair, BigClock, Header, MemGrid, Spark};

pub struct Dash<'a> {
    pub m: &'a Metrics,
    pub theme: &'a Theme,
    pub clock: String,
}

/// Truncate with an ellipsis rather than a hard cut, so a clipped value is
/// visibly clipped instead of silently wrong — "Standard PC (Q35 + ICH9, 2009"
/// reads as a complete string and is not one.
fn ellipsize(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max == 1 {
        return "…".into();
    }
    s.chars().take(max - 1).collect::<String>() + "…"
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.0
}

fn dur(d: std::time::Duration) -> String {
    let s = d.as_secs();
    format!("{}d {}h {}m", s / 86400, (s % 86400) / 3600, (s % 3600) / 60)
}

impl Widget for Dash<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let t = self.theme;
        let w = area.width;
        let mut y = area.y;
        let bottom = area.y + area.height;

        // Every panel checks its own room before drawing. A half-drawn panel
        // is worse than an absent one.
        macro_rules! room {
            ($n:expr) => {
                if y + $n > bottom {
                    return;
                }
            };
        }

        room!(Header::HEIGHT);
        Header::new("PANEL", "SYSTEM", t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT + 1;

        room!(BigClock::HEIGHT);
        BigClock { text: &self.clock, theme: t }
            .render(Rect::new(area.x, y, w, BigClock::HEIGHT), buf);
        y += BigClock::HEIGHT + 1;

        // Uptime / tasks / temp — eDEX's second row, trimmed to what a VM and
        // a laptop can both actually report.
        room!(2);
        let third = w / 3;
        pair(buf, Rect::new(area.x, y, third, 2), "UPTIME", &dur(self.m.uptime), t);
        pair(buf, Rect::new(area.x + third, y, third, 2), "TASKS",
             &self.m.tasks.to_string(), t);
        let temp = self.m.temp_c.map(|c| format!("{c:.0}°C")).unwrap_or_else(|| "—".into());
        pair(buf, Rect::new(area.x + 2 * third, y, w - 2 * third, 2), "TEMP", &temp, t);
        y += 3;

        room!(Header::HEIGHT);
        Header::new("MANUFACTURER", "MODEL", t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;
        room!(1);
        // Vendor left, model right, with a guaranteed gap. A VM reports vendor
        // "QEMU" and model "Standard PC (Q35 + ICH9, 2009)" — without the gap
        // they render flush and read as one run-on string.
        let vend_max = (w as usize).saturating_sub(4) / 2;
        let vendor = ellipsize(&self.m.hardware.vendor, vend_max);
        buf.set_string(area.x, y, &vendor, Style::default().fg(col(t.text)));
        let left = (w as usize).saturating_sub(vendor.chars().count() + 2);
        let model = ellipsize(&self.m.hardware.model, left);
        if !model.is_empty() {
            buf.set_string(area.x + w - model.chars().count() as u16, y, &model,
                           Style::default().fg(col(t.text)));
        }
        y += 2;

        // CPU. This is the panel btop structurally cannot put in a side column.
        room!(Header::HEIGHT);
        let cpu_label: String = self.m.hardware.cpu_model.chars().take(18).collect();
        Header::new("CPU USAGE", &cpu_label, t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;

        let pairs = self.m.per_core.len().div_ceil(2);
        for i in 0..pairs {
            if y + 2 > bottom {
                break;
            }
            let a = &self.m.per_core[i * 2];
            let b = self.m.per_core.get(i * 2 + 1);
            let avg = {
                let va = a.back().copied().unwrap_or(0.0);
                let vb = b.and_then(|h| h.back().copied()).unwrap_or(va);
                (va + vb) / 2.0
            };
            buf.set_string(area.x, y, format!("#{}-{}", i * 2 + 1, i * 2 + 2),
                           Style::default().fg(col(t.text)));
            buf.set_string(area.x, y + 1, format!("{avg:4.0}%"),
                           Style::default().fg(col(t.text_muted)));
            let sx = area.x + 6;
            let sw = w.saturating_sub(6);
            let va: Vec<f32> = a.iter().copied().collect();
            Spark { data: &va, theme: t }.render(Rect::new(sx, y, sw, 1), buf);
            if let Some(hb) = b {
                let vb: Vec<f32> = hb.iter().copied().collect();
                Spark { data: &vb, theme: t }.render(Rect::new(sx, y + 1, sw, 1), buf);
            }
            y += 2;
        }
        y += 1;

        // Memory, with the grid.
        room!(Header::HEIGHT);
        let mem = format!("{:.1}/{:.1} GiB", gib(self.m.memory.used), gib(self.m.memory.total));
        Header::new("MEMORY", &mem, t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;
        let grid_h = 4u16.min(bottom.saturating_sub(y));
        if grid_h > 0 {
            let frac = if self.m.memory.total == 0 { 0.0 }
                       else { self.m.memory.used as f64 / self.m.memory.total as f64 };
            MemGrid { frac, theme: t }.render(Rect::new(area.x, y, w, grid_h), buf);
            y += grid_h + 1;
        }

        // Swap as a single bar — eDEX gives it one row, not a grid.
        if y + 1 <= bottom && self.m.memory.swap_total > 0 {
            let frac = self.m.memory.swap_used as f64 / self.m.memory.swap_total as f64;
            let barw = w.saturating_sub(14) as usize;
            let fill = (frac * barw as f64).round() as usize;
            buf.set_string(area.x, y, "SWAP", Style::default().fg(col(t.text_dim)));
            let bar: String = (0..barw).map(|i| if i < fill { '█' } else { '░' }).collect();
            buf.set_string(area.x + 5, y, &bar, Style::default().fg(col(t.rule)));
            let lbl = format!("{:.1}G", gib(self.m.memory.swap_used));
            buf.set_string(area.x + w - lbl.len() as u16, y, &lbl,
                           Style::default().fg(col(t.text_muted)));
            y += 2;
        }

        room!(Header::HEIGHT);
        Header::new("TOP PROCESSES", "PID · CPU", t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;
        for p in &self.m.procs {
            if y >= bottom {
                break;
            }
            buf.set_string(area.x, y, format!("{:>6}", p.pid),
                           Style::default().fg(col(t.text_muted)));
            let name: String = p.name.chars().take(w.saturating_sub(14) as usize).collect();
            buf.set_string(area.x + 7, y, &name, Style::default().fg(col(t.text)));
            let cpu = format!("{:4.1}%", p.cpu);
            buf.set_string(area.x + w - cpu.len() as u16, y, &cpu,
                           Style::default().fg(col(t.text_dim)));
            y += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ellipsize;

    #[test]
    fn ellipsize_marks_what_it_cuts() {
        assert_eq!(ellipsize("QEMU", 10), "QEMU");
        assert_eq!(ellipsize("Standard PC (Q35 + ICH9, 2009)", 12), "Standard PC…");
        assert_eq!(ellipsize("abc", 0), "");
        assert_eq!(ellipsize("abc", 1), "…");
        // Never panics mid-codepoint: these widgets draw box glyphs and the
        // same class of bug already crashed MemGrid at exactly 34 columns.
        for n in 0..12 {
            let _ = ellipsize("Aspire A515-51G ✓ ▀▀▀", n);
        }
    }
}
