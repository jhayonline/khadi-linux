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

use crate::widgets::{bar, col, ellipsize, grid, BigClock, Graph, Header, MemGrid};

pub struct Dash<'a> {
    pub m: &'a Metrics,
    pub theme: &'a Theme,
    pub clock: String,
}

/// Process memory, in the shortest form that still reads: MiB under a gig,
/// GiB above it. A process list is scanned, not studied.
fn mib(bytes: u64) -> String {
    let m = bytes as f64 / 1024.0 / 1024.0;
    if m >= 1024.0 { format!("{:.1}G", m / 1024.0) } else { format!("{m:.0}M") }
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.0
}

/// Uptime, short enough for a quarter-width cell.
///
/// This was always "0d 6h 18m", which is eleven characters and got cut to
/// "0d 6h 2" the moment the row went from three cells to four. Leading "0d "
/// is noise on a machine that has been up for hours, and the minutes are noise
/// on one that has been up for days.
fn dur(d: std::time::Duration) -> String {
    let s = d.as_secs();
    let (days, hours, mins) = (s / 86400, (s % 86400) / 3600, (s % 3600) / 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
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

        // eDEX's second row is four cells wide, not three. The panel has the
        // width for it once the values are kept short, and a fourth cell is
        // free density — the alternative was the empty third of a row.
        room!(2);
        let temp = self.m.temp_c.map(|c| format!("{c:.0}°C")).unwrap_or_else(|| "—".into());
        let date = chrono::Local::now().format("%d %b").to_string().to_uppercase();
        grid(buf, Rect::new(area.x, y, w, 2), &[
            ("DATE", &date),
            ("UPTIME", &dur(self.m.uptime)),
            ("TASKS", &self.m.tasks.to_string()),
            ("TEMP", &temp),
        ], t);
        y += 3;

        // Three cells: vendor, model, chassis. The old version put vendor and
        // model on ONE line and a VM's "Standard PC (Q35 + ICH9, 2009)" ate it
        // — the ellipsis was the panel telling us the layout was wrong, not the
        // string being too long. `chassis` was already collected and never
        // shown, which is a third of a band for free.
        room!(Header::HEIGHT);
        // The right slot used to read "VENDOR · MODEL", which is the labels of the
        // two cells directly beneath it — chrome restating the content. The
        // hostname is the one fact about this machine that is nowhere else on
        // the screen.
        Header::new("HARDWARE", &self.m.hardware.host, t)
            .render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;
        room!(2);
        let hw = &self.m.hardware;
        grid(buf, Rect::new(area.x, y, w, 2), &[
            ("VENDOR", &hw.vendor),
            ("MODEL", &hw.model),
            ("CHASSIS", &hw.chassis),
        ], t);
        y += 3;

        // CPU. This is the panel btop structurally cannot put in a side column.
        room!(Header::HEIGHT);
        let cpu_label: String = self.m.hardware.cpu_model.chars().take(18).collect();
        Header::new("CPU USAGE", &cpu_label, t).render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;

        // One graph per core PAIR, with both cores drawn in it — which is what
        // eDEX does, and why its CPU block is two graphs rather than four
        // stripes. Three rows each: braille resolves four sub-cells per row, so
        // three rows is where a percentage starts to have a shape.
        let pairs = self.m.per_core.len().div_ceil(2);
        for i in 0..pairs {
            if y + 3 > bottom {
                break;
            }
            let a: Vec<f32> = self.m.per_core[i * 2].iter().copied().collect();
            let b: Vec<f32> = self.m.per_core.get(i * 2 + 1)
                .map(|h| h.iter().copied().collect())
                .unwrap_or_default();
            let avg = {
                let va = a.last().copied().unwrap_or(0.0);
                let vb = b.last().copied().unwrap_or(va);
                (va + vb) / 2.0
            };
            buf.set_string(area.x, y, format!("#{}-{}", i * 2 + 1, i * 2 + 2),
                           Style::default().fg(col(t.text)));
            buf.set_string(area.x, y + 1, format!("{avg:>3.0}%"),
                           Style::default().fg(col(t.text_muted)));
            let gx = area.x + 6;
            let gw = w.saturating_sub(6);
            let series: Vec<&[f32]> = if b.is_empty() { vec![&a] } else { vec![&a, &b] };
            Graph { series: &series, max: 100.0, theme: t }
                .render(Rect::new(gx, y, gw, 3), buf);
            y += 4;
        }

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
            let lbl = format!("{:.1}G", gib(self.m.memory.swap_used));
            let barw = w.saturating_sub(7 + lbl.len() as u16);
            buf.set_string(area.x, y, "SWAP", Style::default().fg(col(t.text_dim)));
            bar(buf, area.x + 5, y, barw, frac, t);
            buf.set_string(area.x + w - lbl.len() as u16, y, &lbl,
                           Style::default().fg(col(t.text_muted)));
            y += 2;
        }

        // Four columns, as eDEX has: PID, NAME, CPU, MEM. `mem` was already
        // collected per process and never drawn.
        room!(Header::HEIGHT);
        // "BY CPU" states the sort order; "PID · CPU · MEM" only named the
        // columns, which are already legible from the values.
        Header::new("TOP PROCESSES", "BY CPU", t)
            .render(Rect::new(area.x, y, w, 2), buf);
        y += Header::HEIGHT;
        for p in &self.m.procs {
            if y >= bottom {
                break;
            }
            buf.set_string(area.x, y, format!("{:>6}", p.pid),
                           Style::default().fg(col(t.text_muted)));
            let cpu = format!("{:4.1}%", p.cpu);
            let mem = format!("{:>5}", mib(p.mem));
            // Name takes whatever the two right-hand columns leave.
            let name_w = w.saturating_sub(7 + cpu.len() as u16 + mem.len() as u16 + 2);
            buf.set_string(area.x + 7, y, ellipsize(&p.name, name_w as usize), Style::default().fg(col(t.text)));
            buf.set_string(area.x + w - mem.len() as u16 - cpu.len() as u16 - 1, y, &cpu,
                           Style::default().fg(col(t.text_dim)));
            buf.set_string(area.x + w - mem.len() as u16, y, &mem,
                           Style::default().fg(col(t.text_muted)));
            y += 1;
        }
    }
}
