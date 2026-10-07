//! The filesystem strip.
//!
//! Runs full-width along the bottom, where eDEX puts FILESYSTEM. It shows
//! mounts and their usage, not a file browser: browsing is yazi's job
//! (`Super+E`), and eDEX's icon grid needs a Nerd Font Khadi deliberately does
//! not ship — Phase 1 measured the tofu that causes on a clean install.

use khadi_core::{disks::Filesystem, net::total, Theme};
use ratatui::{buffer::Buffer, layout::Rect, style::Style, widgets::Widget};

use crate::widgets::{bar, col, Header};

const LABEL_W: u16 = 18;
const PCT_W: u16 = 4;
const GAP: u16 = 2;
const GUTTER: u16 = 2;

pub struct FsPanel<'a> {
    pub fs: &'a Filesystem,
    pub theme: &'a Theme,
}

impl Widget for FsPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let t = self.theme;
        let w = area.width;
        if area.height < Header::HEIGHT + 1 || w < 24 {
            return;
        }
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        Header::new("FILESYSTEM", &cwd, t).render(Rect::new(area.x, area.y, w, 2), buf);

        let mut y = area.y + Header::HEIGHT;
        let bottom = area.y + area.height;

        // Two columns if there is room: a wide bottom strip with one mount per
        // row wastes most of its width.
        let cols: u16 = if w >= 90 { 2 } else { 1 };
        let colw = w / cols;

        for (i, m) in self.fs.mounts.iter().enumerate() {
            let cx = area.x + (i as u16 % cols) * colw;
            let cy = y + (i as u16 / cols);
            if cy >= bottom {
                break;
            }

            let pct = (m.frac() * 100.0).round() as u32;
            let label: String = m.path.chars().take(LABEL_W as usize).collect();
            buf.set_string(cx, cy, &label, Style::default().fg(col(t.text)));

            // Lay the row out right-to-left from measured text, not from a
            // guessed constant: "58.27 GiB / 236.46 GiB" is 22 columns and the
            // first guess reserved 24 including the percentage, so the bar ran
            // straight into it.
            let sizes = format!("{} / {}", total(m.used), total(m.total));
            let sizes_w = sizes.chars().count() as u16;
            let pct_x = cx + colw.saturating_sub(PCT_W + GUTTER);
            let sizes_x = pct_x.saturating_sub(sizes_w + GAP);

            let barx = cx + LABEL_W + GAP;
            let barw = sizes_x.saturating_sub(barx + GAP);
            if barw > 2 {
                bar(buf, barx, cy, barw, m.frac(), t);
            }
            buf.set_string(sizes_x, cy, &sizes, Style::default().fg(col(t.text_muted)));
            let p = format!("{pct:>3}%");
            // A nearly-full mount is the only thing on this strip worth
            // noticing, so it is the only thing at full accent.
            buf.set_string(
                pct_x,
                cy,
                &p,
                Style::default().fg(col(if pct >= 90 { t.text } else { t.text_dim })),
            );
        }
        y += self.fs.mounts.len().div_ceil(cols as usize) as u16;
        let _ = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khadi_core::disks::Filesystem;
    use ratatui::{backend::TestBackend, Terminal};

    fn theme() -> Theme {
        Theme::from_str(
            r##"
[meta]
name = "t"
[role]
text = "#aacfd1"
text_dim = "#89a7aa"
text_muted = "#586c6f"
rule = "#364448"
rule_faint = "#263034"
ground = "#05080d"
[ramp]
a10 = "#161c21"
a20 = "#263034"
a30 = "#364448"
a40 = "#47585b"
a50 = "#586c6f"
a60 = "#687f83"
a70 = "#789396"
a80 = "#89a7aa"
a90 = "#9abbbd"
"##,
        )
        .unwrap()
    }

    /// Columns must never overlap: the first version ran the usage bar
    /// straight through the size text because the reserve was a guess.
    #[test]
    fn columns_do_not_collide_at_any_width() {
        let t = theme();
        let mut fs = Filesystem::new();
        fs.refresh();
        for w in 24u16..=200 {
            let mut term = Terminal::new(TestBackend::new(w, 6)).unwrap();
            term.draw(|f| f.render_widget(FsPanel { fs: &fs, theme: &t }, f.area()))
                .unwrap_or_else(|e| panic!("FsPanel {w}: {e}"));
            let buf = term.backend().buffer().clone();
            for y in 2..6u16 {
                let row: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
                // A bar glyph immediately followed by a digit means the bar
                // overran into the size column.
                assert!(
                    !row.contains("█5") && !row.contains("█9") && !row.contains("█1"),
                    "bar collided with text at width {w}: {row:?}"
                );
            }
        }
    }
}
