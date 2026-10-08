//! The chrome the login screen draws.
//!
//! These three widgets used to live in `khadi-hud`, which the greeter depended
//! on so that the clock here and the clock on the system panel could not drift
//! apart. `khadi-hud` is gone — the desktop's panels are `khadi-shell` now —
//! and the greeter is the one surface that cannot follow it there: it runs on
//! a VT before a session exists, where there is no compositor to put a webview
//! on. So the widgets come with it.
//!
//! There is nothing left to drift against. This is the only implementation.

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
/// ```text
/// PANEL                    SYSTEM
/// ┬──────────────────────────────┬
/// ```
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
    #[allow(dead_code)] // kept so the two-row requirement stays stated
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

/// Five-row seven-segment digits, one cell per stroke, drawn solid.
///
/// THE STROKE WEIGHT IS THE DESIGN. This was a 5x5 face of solid blocks, and
/// against eDEX it read as crude: eDEX's clock is the most prominent thing on
/// its screen and it is drawn in *thin* strokes, so it carries size without
/// carrying weight. Five-cell strokes are a slab; one-cell strokes are a
/// readout.
///
/// IT ALSO BUYS BACK THE SECONDS. Phase 0 locked `HH:MM` after measuring eight
/// glyphs of the solid face at 39 columns against a 34-column panel. The
/// measurement was right and the conclusion was about the wrong variable: the
/// cost was the *width of the face*, not the number of glyphs. At 3 columns a
/// digit and 1 for the colon, `HH:MM:SS` is 27 columns and fits with room to
/// spare.
pub struct BigClock<'a> {
    pub text: &'a str,
    pub theme: &'a Theme,
}

impl BigClock<'_> {
    pub const HEIGHT: u16 = 5;

    /// Segments, in the order a b c d e f g — the standard seven-segment
    /// lettering, so the table below can be read against any reference.
    fn segments(c: char) -> [bool; 7] {
        //                      a      b      c      d      e      f      g
        match c {
            '0' => [true,  true,  true,  true,  true,  true,  false],
            '1' => [false, true,  true,  false, false, false, false],
            '2' => [true,  true,  false, true,  true,  false, true ],
            '3' => [true,  true,  true,  true,  false, false, true ],
            '4' => [false, true,  true,  false, false, true,  true ],
            '5' => [true,  false, true,  true,  false, true,  true ],
            '6' => [true,  false, true,  true,  true,  true,  true ],
            '7' => [true,  true,  true,  false, false, false, false],
            '8' => [true,  true,  true,  true,  true,  true,  true ],
            '9' => [true,  true,  true,  true,  false, true,  true ],
            _   => [false; 7],
        }
    }

    /// One glyph row, built from the segments with the CORNERS RESOLVED.
    ///
    /// The first cut drew each stroke on its own — a `───` bar on one row and
    /// two `│` on the next — and the strokes never met. At three cells wide
    /// every junction is a corner, so a glyph drawn without corners is five
    /// disconnected fragments and a screenshot of it reads as noise rather
    /// than as a time. Each cell here asks which segments meet at it and picks
    /// the box glyph that joins them.
    fn glyph(c: char) -> [String; 5] {
        // The colon is one column, not three: a 3-wide colon pads the clock
        // out to where the seconds will not fit, which is how the seconds got
        // dropped in the first place. One cell on all five rows, or every
        // column after it shifts.
        if c == ':' {
            return [" ", "\u{25aa}", " ", "\u{25aa}", " "].map(String::from);
        }
        let s = Self::segments(c);
        let (a, b, cc, d, e, f, g) = (s[0], s[1], s[2], s[3], s[4], s[5], s[6]);
        // SOLID STROKES, NOT HAIRLINES. The face was drawn with `─` and `│`,
        // which put a two-pixel line down the middle of a twenty-pixel cell:
        // next to eDEX's clock, whose strokes are a tenth of the glyph height,
        // it read as a wireframe of a clock rather than as a readout. A full
        // block fills its cell, so the stroke becomes the cell — and the
        // corner-joining the hairline version needed disappears with it,
        // because two solid cells meeting ARE a corner.
        let v = |on: bool| if on { '\u{2588}' } else { ' ' };
        let h = |on: bool| if on { "\u{2588}\u{2588}\u{2588}" } else { "   " };
        let tl = v(a || f);
        let tr = v(a || b);
        let ml = v(g || f || e);
        let mr = v(g || b || cc);
        let bl = v(d || e);
        let br = v(d || cc);
        [
            format!("{tl}{}{tr}", h(a)),
            format!("{}   {}", v(f), v(b)),
            format!("{ml}{}{mr}", h(g)),
            format!("{}   {}", v(e), v(cc)),
            format!("{bl}{}{br}", h(d)),
        ]
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
                line.push_str(&Self::glyph(c)[row as usize]);
            }
            let line: String = line.chars().take(area.width as usize).collect();
            // Centred: the face is narrower than the panel now, and a readout
            // pinned to the left edge reads as an accident.
            let pad = (area.width as usize).saturating_sub(line.chars().count()) / 2;
            buf.set_string(area.x + pad as u16, area.y + row, &line, style);
        }
    }
}
