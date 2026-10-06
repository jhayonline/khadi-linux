//! The eDEX motifs, as ratatui widgets.
//!
//! These are the reason khadi-hud exists. Phase 0 measured that no off-the-shelf
//! TUI can draw them: zellij's frame glyphs are a closed enum, and btop refuses
//! to render its CPU box below 60 columns. Everything here is built to work in
//! a 34-column side panel, which is eDEX's actual proportion.

use khadi_core::{Rgb, Theme};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

pub fn col(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// The signature motif: an uppercase label pair over a hairline that
/// terminates in a tick at each end.
///
///     PANEL                    SYSTEM
///     ┬──────────────────────────────┬
///
/// Phase 0 measured both variants. The two-row form is faithful to the CSS
/// (`╷ … ╷` on its own line) but costs 43% of screen height across nine
/// headers; the one-row `┬───┬` costs 23%. In CSS the tick is sub-pixel
/// decoration; in a cell grid it is a whole row that could have held a
/// process. So the terminal surface uses one row and the GTK panel, where it
/// costs nothing, uses the faithful geometry.
pub struct Header<'a> {
    pub left: &'a str,
    pub right: &'a str,
    pub theme: &'a Theme,
}

impl<'a> Header<'a> {
    pub const HEIGHT: u16 = 2;

    pub fn new(left: &'a str, right: &'a str, theme: &'a Theme) -> Self {
        Self { left, right, theme }
    }
}

impl Widget for Header<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 2 || area.width < 4 {
            return;
        }
        let w = area.width as usize;
        let text = Style::default().fg(col(self.theme.text));
        let dim = Style::default().fg(col(self.theme.text_dim));
        let rule = Style::default().fg(col(self.theme.rule));

        // Label pair: left flush, right flush, truncated rather than wrapped —
        // a wrapped label breaks the single-row grid the whole design rests on.
        let l: String = self.left.chars().take(w).collect();
        buf.set_string(area.x, area.y, &l, text);
        let avail = w.saturating_sub(l.chars().count() + 1);
        let r: String = self.right.chars().take(avail).collect();
        if !r.is_empty() {
            let x = area.x + area.width - r.chars().count() as u16;
            buf.set_string(x, area.y, &r, dim);
        }

        // The ticked rule.
        let mut line = String::with_capacity(w);
        line.push('┬');
        for _ in 0..w.saturating_sub(2) {
            line.push('─');
        }
        if w >= 2 {
            line.push('┬');
        }
        buf.set_string(area.x, area.y + 1, &line, rule);
    }
}

/// CPU history as block glyphs. eDEX draws a thin line graph; a cell grid
/// cannot, so the eight block heights are the honest approximation.
pub struct Spark<'a> {
    pub data: &'a [f32],
    pub theme: &'a Theme,
}

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

impl Widget for Spark<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let w = area.width as usize;
        // Right-align: the newest sample belongs at the right edge, so a
        // partially-filled history grows leftward instead of jumping.
        let take = self.data.len().min(w);
        let slice = &self.data[self.data.len() - take..];
        let pad = w - take;
        let mut s = String::with_capacity(w);
        for _ in 0..pad {
            s.push(' ');
        }
        for v in slice {
            let i = ((v / 100.0).clamp(0.0, 1.0) * 7.0).round() as usize;
            s.push(BLOCKS[i.min(7)]);
        }
        buf.set_string(area.x, area.y, &s, Style::default().fg(col(self.theme.text)));
    }
}

/// eDEX's memory block: a grid of cells, filled in proportion to usage. It is
/// the most distinctive element of the left column and nothing off the shelf
/// draws anything like it.
pub struct MemGrid<'a> {
    pub frac: f64,
    pub theme: &'a Theme,
}

impl Widget for MemGrid<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let cells = area.width as usize * area.height as usize;
        let used = (self.frac.clamp(0.0, 1.0) * cells as f64).round() as usize;
        let on = Style::default().fg(col(self.theme.text));
        let off = Style::default().fg(col(self.theme.a(20)));
        for y in 0..area.height {
            // Cells are multi-byte UTF-8, so runs are collected as chars and
            // never sliced by byte index. Byte-slicing a String of box glyphs
            // panics the moment a run boundary lands mid-codepoint.
            let mut start = 0usize;
            let w = area.width as usize;
            while start < w {
                let lit = (y as usize * w + start) < used;
                let mut end = start;
                while end < w && ((y as usize * w + end) < used) == lit {
                    end += 1;
                }
                let seg: String = std::iter::repeat(if lit { '▀' } else { '·' })
                    .take(end - start)
                    .collect();
                buf.set_string(area.x + start as u16, area.y + y, &seg,
                               if lit { on } else { off });
                start = end;
            }
        }
    }
}

/// Five-row block digits. Phase 0 locked `HH:MM` rather than `HH:MM:SS`:
/// eight glyphs at a legible width need 39 columns and the panel is 34, which
/// forced 3-wide digits that read as mush. Dropping seconds buys the visual
/// weight back — 29 of 34 columns at 5-wide.
pub struct BigClock<'a> {
    pub text: &'a str,
    pub theme: &'a Theme,
}

impl BigClock<'_> {
    pub const HEIGHT: u16 = 5;

    fn glyph(c: char) -> [&'static str; 5] {
        match c {
            '0' => ["█████", "█   █", "█   █", "█   █", "█████"],
            '1' => ["   ██", "   ██", "   ██", "   ██", "   ██"],
            '2' => ["█████", "    █", "█████", "█    ", "█████"],
            '3' => ["█████", "    █", "█████", "    █", "█████"],
            '4' => ["█   █", "█   █", "█████", "    █", "    █"],
            '5' => ["█████", "█    ", "█████", "    █", "█████"],
            '6' => ["█████", "█    ", "█████", "█   █", "█████"],
            '7' => ["█████", "    █", "    █", "    █", "    █"],
            '8' => ["█████", "█   █", "█████", "█   █", "█████"],
            '9' => ["█████", "█   █", "█████", "    █", "█████"],
            ':' => ["     ", "  █  ", "     ", "  █  ", "     "],
            _ => ["     ", "     ", "     ", "     ", "     "],
        }
    }
}

impl Widget for BigClock<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < Self::HEIGHT {
            return;
        }
        let style = Style::default().fg(col(self.theme.text));
        for row in 0..5u16 {
            let mut line = String::new();
            for (i, c) in self.text.chars().enumerate() {
                if i > 0 {
                    line.push(' ');
                }
                line.push_str(Self::glyph(c)[row as usize]);
            }
            let line: String = line.chars().take(area.width as usize).collect();
            buf.set_string(area.x, area.y + row, &line, style);
        }
    }
}

/// A label-over-value pair, the unit eDEX's info grids are built from.
pub fn pair(buf: &mut Buffer, area: Rect, label: &str, value: &str, theme: &Theme) {
    if area.height < 2 {
        return;
    }
    let w = area.width as usize;
    buf.set_string(
        area.x,
        area.y,
        label.chars().take(w).collect::<String>(),
        Style::default().fg(col(theme.text_dim)),
    );
    buf.set_string(
        area.x,
        area.y + 1,
        value.chars().take(w).collect::<String>(),
        Style::default().fg(col(theme.text)),
    );
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

    /// Every widget here draws multi-byte box glyphs. Slicing those by byte
    /// index panics, and it only shows up at specific widths — so sweep.
    #[test]
    fn widgets_survive_every_narrow_width() {
        let t = theme();
        for w in 1u16..=40 {
            for h in 1u16..=8 {
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| {
                    let a = f.area();
                    f.render_widget(MemGrid { frac: 0.37, theme: &t }, a);
                })
                .unwrap_or_else(|e| panic!("MemGrid {w}x{h}: {e}"));

                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| {
                    f.render_widget(Header::new("PANEL", "SYSTEM", &t), f.area());
                })
                .unwrap_or_else(|e| panic!("Header {w}x{h}: {e}"));

                let data: Vec<f32> = (0..60).map(|i| i as f32).collect();
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| {
                    f.render_widget(Spark { data: &data, theme: &t }, f.area());
                })
                .unwrap_or_else(|e| panic!("Spark {w}x{h}: {e}"));
            }
        }
    }

    #[test]
    fn header_draws_the_ticked_rule() {
        let t = theme();
        let mut term = Terminal::new(TestBackend::new(20, 2)).unwrap();
        term.draw(|f| f.render_widget(Header::new("A", "B", &t), f.area())).unwrap();
        let buf = term.backend().buffer();
        let rule: String = (0..20).map(|x| buf[(x, 1)].symbol().to_string()).collect();
        assert!(rule.starts_with('┬'), "rule must open with a tick: {rule:?}");
        assert!(rule.ends_with('┬'), "rule must close with a tick: {rule:?}");
        assert_eq!(buf[(0, 0)].symbol(), "A");
        assert_eq!(buf[(19, 0)].symbol(), "B", "right label is flush right");
    }

    /// The clock was locked to HH:MM in Phase 0 because HH:MM:SS needs 39
    /// columns at a legible glyph width and the panel is 34.
    #[test]
    fn clock_fits_a_34_column_panel() {
        let t = theme();
        let mut term = Terminal::new(TestBackend::new(34, 5)).unwrap();
        term.draw(|f| f.render_widget(BigClock { text: "12:34", theme: &t }, f.area())).unwrap();
        let buf = term.backend().buffer();
        let row: String = (0..34).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert!(row.contains('█'), "clock did not draw");
        assert!(row.trim_end().chars().count() <= 34);
    }
}
