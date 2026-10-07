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
    symbols::Marker,
    widgets::{
        canvas::{Canvas, Line as CanvasLine},
        Widget,
    },
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

/// eDEX's memory block: a grid of cells, filled in proportion to usage. It is
/// the most distinctive element of the left column and nothing off the shelf
/// draws anything like it.
/// One or more series as polylines, on a braille canvas.
///
/// eDEX draws its CPU and traffic histories as smooth curves; the bar
/// sparkline this replaces is a different instrument — it reads as a tally,
/// not a trace, and at one row it quantises a percentage into eight steps.
/// Braille gives four vertical sub-cells per row, so three rows resolve the
/// same history into roughly twelve, and the line actually has a shape.
///
/// Series are drawn back to front in descending ramp steps, so an overlapping
/// pair stays readable without a second colour — the one-colour palette has no
/// second hue to spend (PLAN.md section 3).
pub struct Graph<'a> {
    pub series: &'a [&'a [f32]],
    pub max: f32,
    pub theme: &'a Theme,
}

impl Widget for Graph<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 2 || area.height == 0 || self.series.is_empty() {
            return;
        }
        let n = self.series.iter().map(|s| s.len()).max().unwrap_or(0);
        if n < 2 {
            return;
        }
        let max = self.max.max(1.0) as f64;
        let shades = [self.theme.text, self.theme.a(60), self.theme.a(40)];
        Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([0.0, (n - 1) as f64])
            .y_bounds([0.0, max])
            .paint(|ctx| {
                for (si, data) in self.series.iter().enumerate().rev() {
                    let shade = col(shades[si.min(shades.len() - 1)]);
                    // Right-align short histories: a buffer that has not filled
                    // yet should grow from the right like a trace, not sit at
                    // the left like a stub.
                    let off = n - data.len();
                    for (i, pair) in data.windows(2).enumerate() {
                        ctx.draw(&CanvasLine {
                            x1: (off + i) as f64,
                            y1: pair[0].clamp(0.0, max as f32) as f64,
                            x2: (off + i + 1) as f64,
                            y2: pair[1].clamp(0.0, max as f32) as f64,
                            color: shade,
                        });
                    }
                }
            })
            .render(area, buf);
    }
}

pub struct MemGrid<'a> {
    pub frac: f64,
    pub theme: &'a Theme,
}

impl Widget for MemGrid<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 2 || area.height == 0 {
            return;
        }
        // DISCRETE CELLS, ONE COLUMN APART. Drawn edge to edge these merge into
        // solid runs and the band reads as a stacked bar chart; eDEX's memory
        // block reads as a matrix because you can see the individual cells.
        // The gutter is the whole effect.
        let per_row = (area.width as usize + 1) / 2;
        let cells = per_row * area.height as usize;
        let used = (self.frac.clamp(0.0, 1.0) * cells as f64).round() as usize;
        let on = Style::default().fg(col(self.theme.text));
        let off = Style::default().fg(col(self.theme.a(20)));
        for y in 0..area.height {
            for i in 0..per_row {
                let lit = (y as usize * per_row + i) < used;
                buf.set_string(
                    area.x + (i * 2) as u16,
                    area.y + y,
                    if lit { "\u{25aa}" } else { "\u{00b7}" },
                    if lit { on } else { off },
                );
            }
        }
    }
}

/// Five-row seven-segment digits, one cell per stroke.
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
        let tl = match (a, f) {
            (true, true) => '┌',
            (true, false) => '─',
            (false, true) => '│',
            _ => ' ',
        };
        let tr = match (a, b) {
            (true, true) => '┐',
            (true, false) => '─',
            (false, true) => '│',
            _ => ' ',
        };
        // The middle row is the only three-way junction: the crossbar can meet
        // the vertical above, below, or both.
        let ml = match (g, f, e) {
            (true, true, true) => '├',
            (true, true, false) => '└',
            (true, false, true) => '┌',
            (true, false, false) => '─',
            (false, true, _) | (false, _, true) => '│',
            _ => ' ',
        };
        let mr = match (g, b, cc) {
            (true, true, true) => '┤',
            (true, true, false) => '┘',
            (true, false, true) => '┐',
            (true, false, false) => '─',
            (false, true, _) | (false, _, true) => '│',
            _ => ' ',
        };
        let bl = match (d, e) {
            (true, true) => '└',
            (true, false) => '─',
            (false, true) => '│',
            _ => ' ',
        };
        let br = match (d, cc) {
            (true, true) => '┘',
            (true, false) => '─',
            (false, true) => '│',
            _ => ' ',
        };
        let v = |on: bool| if on { '│' } else { ' ' };
        let h = |on: bool| if on { '─' } else { ' ' };
        [
            format!("{tl}{}{tr}", h(a)),
            format!("{} {}", v(f), v(b)),
            format!("{ml}{}{mr}", h(g)),
            format!("{} {}", v(e), v(cc)),
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

/// A label-over-value pair, the unit eDEX's info grids are built from.
/// Several label-over-value cells across one band.
///
/// eDEX's panels are built almost entirely from these, and it is how a narrow
/// column carries four readouts without any of them wrapping: four short cells
/// beat one long line, because the long line is what has to be truncated.
/// Each cell reserves a column of gutter so neighbours cannot run together.
pub fn grid(buf: &mut Buffer, area: Rect, cells: &[(&str, &str)], theme: &Theme) {
    if area.height < 2 || cells.is_empty() {
        return;
    }
    let n = cells.len() as u16;
    let even = area.width / n;
    if even < 3 {
        return;
    }
    // Equal columns while everything fits, because an even band is the tidier
    // one and the four-cell DATE/UPTIME/TASKS/TEMP row is the common case.
    // When a cell does not fit, widen it from its neighbours' slack rather
    // than truncating — the network band was cutting "192.168.100.58" to
    // "192.168.100", which is not an address and does not read as a clipped
    // one either.
    let need: Vec<u16> = cells
        .iter()
        .map(|(l, v)| l.chars().count().max(v.chars().count()) as u16 + 1)
        .collect();
    let widths: Vec<u16> = if need.iter().all(|&x| x <= even) {
        (0..n).map(|i| if i == n - 1 { area.width - i * even } else { even }).collect()
    } else {
        // Water-fill: hand out an equal share, let any cell that wants less
        // than its share take only what it needs, and pass the remainder on.
        // Falling back to equal columns instead cut "Standard PC (Q35 + ICH9,
        // 2009)" to "Standard …" while VENDOR sat on four unused columns —
        // the band had the room, it was just divided by the wrong rule.
        let mut w = vec![0u16; cells.len()];
        let mut order: Vec<usize> = (0..cells.len()).collect();
        order.sort_by_key(|&i| need[i]);
        let mut left = area.width;
        for (k, &i) in order.iter().enumerate() {
            let share = left / (cells.len() - k) as u16;
            w[i] = need[i].min(share);
            left -= w[i];
        }
        // Anything still unclaimed goes to the hungriest cell.
        if let Some(&i) = order.last() {
            w[i] += left;
        }
        w
    };
    let mut x = area.x;
    for ((label, value), cw) in cells.iter().zip(widths) {
        pair(buf, Rect::new(x, area.y, cw.saturating_sub(1), 2), label, value, theme);
        x += cw;
    }
}

/// A two-tone meter: one glyph, two weights.
///
/// Every bar on the screen used `█` over `░` in a single colour, which is two
/// textures doing the job one value change does better — the dither reads as
/// noise at a distance and the two halves were the same brightness, so the
/// fill level was carried entirely by a glyph shape four pixels wide. Same
/// glyph throughout, lit against unlit, is the vocabulary the memory grid
/// already uses; now the whole design system shares it.
/// Truncate with an ellipsis rather than a hard cut, so a clipped value is
/// visibly clipped instead of silently wrong — "Standard PC (Q35 + ICH9, 2009"
/// reads as a complete string and is not one.
pub fn ellipsize(s: &str, max: usize) -> String {
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

pub fn bar(buf: &mut Buffer, x: u16, y: u16, w: u16, frac: f64, theme: &Theme) {
    if w == 0 {
        return;
    }
    let fill = (frac.clamp(0.0, 1.0) * w as f64).round() as u16;
    let lit = Style::default().fg(col(theme.text));
    let unlit = Style::default().fg(col(theme.a(20)));
    if fill > 0 {
        buf.set_string(x, y, "\u{2588}".repeat(fill as usize), lit);
    }
    if fill < w {
        buf.set_string(x + fill, y, "\u{2588}".repeat((w - fill) as usize), unlit);
    }
}

pub fn pair(buf: &mut Buffer, area: Rect, label: &str, value: &str, theme: &Theme) {
    if area.height < 2 {
        return;
    }
    // Ellipsised, not cut. A hard cut turns "Standard PC (Q35 + ICH9, 2009)"
    // into "Standard PC", which reads as a complete string and is not one.
    let w = area.width as usize;
    buf.set_string(area.x, area.y, ellipsize(label, w),
                   Style::default().fg(col(theme.text_dim)));
    buf.set_string(area.x, area.y + 1, ellipsize(value, w),
                   Style::default().fg(col(theme.text)));
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

                // Graph gets the same sweep the sparkline had: it draws on a
                // braille canvas, and a canvas with a degenerate area or a
                // single-point series is exactly where that panics.
                let a: Vec<f32> = (0..60).map(|i| i as f32).collect();
                let b: Vec<f32> = (0..7).map(|i| (i * 9) as f32).collect();
                let series: Vec<&[f32]> = vec![&a, &b];
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| {
                    f.render_widget(Graph { series: &series, max: 100.0, theme: &t }, f.area());
                })
                .unwrap_or_else(|e| panic!("Graph {w}x{h}: {e}"));

                // And the degenerate cases by name, since the sweep only
                // varies the area.
                let one: Vec<f32> = vec![42.0];
                let none: Vec<f32> = vec![];
                for s in [&one, &none] {
                    let series: Vec<&[f32]> = vec![s];
                    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                    term.draw(|f| {
                        f.render_widget(Graph { series: &series, max: 100.0, theme: &t }, f.area());
                    })
                    .unwrap_or_else(|e| panic!("Graph {w}x{h} short series: {e}"));
                }
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

    /// The header needs two rows and silently draws nothing with one, which
    /// is how the TERMINAL pane shipped blank the first time: zellij's
    /// horizontal divider ate a row and `size=2` left a one-row pty. The
    /// layout now asks for three. This pins the boundary so the next pane that
    /// hosts a header is sized against a stated requirement.
    #[test]
    fn a_one_row_header_draws_nothing() {
        let t = theme();
        let mut term = Terminal::new(TestBackend::new(20, 1)).unwrap();
        term.draw(|f| f.render_widget(Header::new("A", "B", &t), f.area())).unwrap();
        let buf = term.backend().buffer();
        let row: String = (0..20).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        assert_eq!(row.trim(), "", "half a header is worse than none: {row:?}");
        assert_eq!(Header::HEIGHT, 2, "the pane sizes in the layout depend on this");
    }

    /// The clock was locked to HH:MM in Phase 0 because HH:MM:SS needs 39
    /// columns at a legible glyph width and the panel is 34.
    #[test]
    fn clock_fits_a_34_column_panel() {
        let t = theme();
        let mut term = Terminal::new(TestBackend::new(34, 5)).unwrap();
        // WITH SECONDS. Phase 0 could not fit eight glyphs and dropped them;
        // the seven-segment face costs 3 columns a digit and 1 for the colon,
        // so this is the assertion that decision turned on.
        term.draw(|f| f.render_widget(BigClock { text: "12:34:56", theme: &t }, f.area()))
            .unwrap();
        let buf = term.backend().buffer();
        let mut lit = 0usize;
        let (mut first, mut last) = (34usize, 0usize);
        for y in 0..5u16 {
            for x in 0..34u16 {
                if !buf[(x, y)].symbol().trim().is_empty() {
                    lit += 1;
                    first = first.min(x as usize);
                    last = last.max(x as usize);
                }
            }
        }
        assert!(lit > 40, "clock did not draw (only {lit} cells)");
        let span = last - first + 1;
        assert!(span <= 34, "clock spans {span} columns, panel is 34");
        // Centred, so a face narrower than the panel does not look like it
        // fell against the left edge.
        let left = first;
        let right = 34 - 1 - last;
        assert!(left.abs_diff(right) <= 1, "not centred: {left} left, {right} right");
    }


    /// Every row of every glyph is the same width, or the columns after it
    /// shear. This caught a two-cell colon on rows 0, 2 and 4.
    #[test]
    fn every_clock_glyph_row_is_the_same_width() {
        for c in "0123456789".chars() {
            for (i, row) in BigClock::glyph(c).iter().enumerate() {
                assert_eq!(row.chars().count(), 3, "glyph {c:?} row {i} is {row:?}");
            }
        }
        for (i, row) in BigClock::glyph(':').iter().enumerate() {
            assert_eq!(row.chars().count(), 1, "colon row {i} is {row:?}");
        }
    }

    /// The strokes have to MEET. A digit drawn as unjoined bars and verticals
    /// renders as five fragments: every lit cell on the top and bottom rows of
    /// a closed digit must be a corner or a bar, never a bare vertical.
    #[test]
    fn clock_digits_are_drawn_with_corners() {
        // 0 and 8 are closed top and bottom, so every outer row is corners.
        for c in ['0', '8'] {
            let g = BigClock::glyph(c);
            assert_eq!(g[0], "\u{250c}\u{2500}\u{2510}", "{c:?} top");
            assert_eq!(g[4], "\u{2514}\u{2500}\u{2518}", "{c:?} bottom");
        }
        // 1 is the one digit with no horizontal at all.
        assert!(BigClock::glyph('1').iter().all(|r| r == "  \u{2502}"));
        // 4's crossbar leaves the upper-left vertical and meets both the upper
        // and lower right ones: └─┤, which only a corner-aware builder draws.
        assert_eq!(BigClock::glyph('4')[2], "\u{2514}\u{2500}\u{2524}");
    }

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


    /// A long value borrows from its neighbours' slack. Equal columns cut
    /// "Standard PC (Q35 + ICH9, 2009)" to "Standard …" while VENDOR sat on
    /// four unused ones.
    #[test]
    fn a_grid_cell_borrows_slack_before_it_truncates() {
        let th = theme();
        let mut term = Terminal::new(TestBackend::new(34, 2)).unwrap();
        term.draw(|f| {
            let a = f.area();
            grid(f.buffer_mut(), a, &[
                ("VENDOR", "QEMU"),
                ("MODEL", "Standard PC (Q35 + ICH9, 2009)"),
                ("CHASSIS", "Other"),
            ], &th);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let row: String = (0..34).map(|x| buf[(x, 1)].symbol().to_string()).collect();
        assert!(row.starts_with("QEMU"), "{row:?}");
        assert!(row.contains("Standard PC (Q3"), "model did not get the slack: {row:?}");
        assert!(row.trim_end().ends_with("Other"), "{row:?}");
    }

    /// And when everything fits, the columns stay EVEN — the four-cell
    /// DATE/UPTIME/TASKS/TEMP band is the common case and a ragged one reads
    /// as a bug.
    #[test]
    fn a_grid_that_fits_keeps_even_columns() {
        let th = theme();
        let mut term = Terminal::new(TestBackend::new(32, 2)).unwrap();
        term.draw(|f| {
            let a = f.area();
            grid(f.buffer_mut(), a, &[
                ("DATE", "07 OCT"), ("UPTIME", "6h 44m"),
                ("TASKS", "909"), ("TEMP", "56C"),
            ], &th);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let row: String = (0..32).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        for (i, label) in ["DATE", "UPTIME", "TASKS", "TEMP"].iter().enumerate() {
            assert_eq!(&row[i * 8..i * 8 + label.len()], *label, "cell {i} is not on the grid");
        }
    }

}
