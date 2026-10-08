//! The FILESYSTEM panel.
//!
//! eDEX's is a file BROWSER: an icon grid of the current directory, with a
//! `Mount /home/x used 71%` bar beneath it, occupying the bottom-left quarter
//! of the screen. Khadi shipped only the bar, as a five-row strip, on the
//! reasoning in PLAN.md section 12 — that browsing is yazi's job and an icon
//! grid needs a Nerd Font Khadi does not ship.
//!
//! Half of that still holds: `Super+E` is still yazi, because this panel does
//! not navigate. The Nerd Font half was wrong. eDEX's icons are six
//! silhouettes — a tab over a body, a page with a cut corner, a hollow chain —
//! and drawing shapes a cell grid can hold is the same answer `khadi-hud`
//! already gives for the clock, the globe and the memory block.
//!
//! It follows the shell rather than taking a path, because eDEX's does: the
//! browser and the terminal show the same directory, and a file list that
//! disagrees with the prompt above it is worse than no file list.

use khadi_core::{browse::{Browser, Kind}, net::total, Theme};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};

use crate::widgets::{bar, col, dot_grid, ellipsize, Header, Icon, IconKind};

/// One grid cell: the icon, then the name under it, then a row of air.
const CELL_W: u16 = 11;
const CELL_H: u16 = 5;

pub struct FsPanel<'a> {
    pub fs: &'a Browser,
    pub theme: &'a Theme,
}

fn icon_kind(k: Kind) -> IconKind {
    match k {
        Kind::Up => IconKind::Up,
        Kind::Dir => IconKind::Dir,
        Kind::File => IconKind::File,
        Kind::Link => IconKind::Link,
        Kind::Other => IconKind::Other,
    }
}

impl Widget for FsPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let t = self.theme;
        let w = area.width;
        if area.height < Header::HEIGHT + 2 || w < CELL_W {
            return;
        }

        let cwd = self.fs.cwd.to_string_lossy().to_string();
        Header::new("FILESYSTEM", &cwd, t).render(Rect::new(area.x, area.y, w, 2), buf);

        let top = area.y + Header::HEIGHT;
        let bottom = area.y + area.height;
        // The usage bar owns the last two rows, as eDEX's does.
        let grid_bottom = bottom.saturating_sub(2);

        // The lattice first, so every glyph after it draws over the top.
        if grid_bottom > top {
            dot_grid(buf, Rect::new(area.x, top, w, grid_bottom - top), t);
        }

        let cols = (w / CELL_W).max(1);
        let rows = (grid_bottom.saturating_sub(top)) / CELL_H;
        let capacity = (cols * rows) as usize;

        for (i, e) in self.fs.entries.iter().take(capacity).enumerate() {
            let cx = area.x + (i as u16 % cols) * CELL_W;
            let cy = top + (i as u16 / cols) * CELL_H;

            // Icon centred in the cell.
            let ix = cx + (CELL_W - Icon::W) / 2;
            Icon { kind: icon_kind(e.kind), theme: t, dim: e.hidden }
                .render(Rect::new(ix, cy, Icon::W, Icon::H), buf);

            // Name centred under it, one line, ellipsised. eDEX underlines
            // symlinks and fades dotfiles; both are free here.
            let name = ellipsize(&e.name, (CELL_W - 1) as usize);
            let nx = cx + (CELL_W.saturating_sub(name.chars().count() as u16)) / 2;
            let mut style = Style::default().fg(col(if e.hidden { t.text_muted } else { t.text }));
            if e.kind == Kind::Link {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            buf.set_string(nx, cy + Icon::H, &name, style);
        }

        // An empty directory should say so rather than look broken.
        if self.fs.entries.len() <= 1 && grid_bottom > top {
            buf.set_string(area.x, top + 1, "empty directory",
                           Style::default().fg(col(t.text_muted)));
        }

        // `Mount /home/x used 71%` and the bar, exactly where eDEX puts it.
        if self.fs.total == 0 {
            return;
        }
        let y = bottom - 1;
        let frac = self.fs.used as f64 / self.fs.total as f64;
        let pct = (frac * 100.0).round() as u32;
        let lead = format!("MOUNT {} ", self.fs.mount);
        let tail = format!(" {} / {}  {pct:>3}%", total(self.fs.used), total(self.fs.total));
        buf.set_string(area.x, y, ellipsize(&lead, w as usize),
                       Style::default().fg(col(t.text_dim)));
        let lw = lead.chars().count() as u16;
        let tw = tail.chars().count() as u16;
        if w > lw + tw + 4 {
            bar(buf, area.x + lw, y, w - lw - tw, frac, t);
        }
        if w > tw {
            buf.set_string(area.x + w - tw, y, &tail,
                           Style::default().fg(col(if pct >= 90 { t.text } else { t.text_muted })));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// Every glyph here is multi-byte. Slicing those by byte index panics, and
    /// it only shows up at specific sizes — so sweep both axes.
    #[test]
    fn survives_every_size() {
        let t = theme();
        let mut fs = Browser::new();
        fs.refresh();
        for w in 1u16..=160 {
            for h in [1u16, 2, 3, 4, 5, 8, 12, 20, 24] {
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| f.render_widget(FsPanel { fs: &fs, theme: &t }, f.area()))
                    .unwrap_or_else(|e| panic!("FsPanel {w}x{h}: {e}"));
            }
        }
    }

    /// Every icon row must be the same width, or the column after it shifts —
    /// the bug that made the clock unreadable, one layer down.
    #[test]
    fn every_icon_row_is_the_icon_width() {
        for k in [IconKind::Up, IconKind::Dir, IconKind::File, IconKind::Link, IconKind::Other] {
            for (i, row) in Icon::rows(k).iter().enumerate() {
                assert_eq!(row.chars().count(), Icon::W as usize,
                           "icon row {i} is {row:?}");
            }
        }
    }

    /// The grid has to actually draw icons, not just the lattice behind them.
    #[test]
    fn draws_an_icon_grid() {
        let t = theme();
        let mut fs = Browser::new();
        fs.refresh();
        let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
        term.draw(|f| f.render_widget(FsPanel { fs: &fs, theme: &t }, f.area())).unwrap();
        let buf = term.backend().buffer();
        let mut solid = 0;
        for y in 0..20u16 {
            for x in 0..120u16 {
                if buf[(x, y)].symbol() == "\u{2588}" {
                    solid += 1;
                }
            }
        }
        assert!(solid > 40, "only {solid} filled cells — the icons did not draw");
    }
}
