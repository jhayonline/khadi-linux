//! What the lock screen looks like, drawn a pixel at a time.
//!
//! No font and no toolkit. Everything here is rectangles, which means the lock
//! screen has nothing to load and nothing that can be missing: a machine with no
//! fonts installed, or a `fontconfig` cache that has not been rebuilt, still gets a
//! lock screen rather than a black rectangle. The Khadi wordmark belongs here and is
//! not here yet, because it needs a glyph renderer and that is a dependency this
//! program has to earn.

use crate::theme::Theme;

/// What the screen is saying.
#[derive(Clone, Copy, PartialEq)]
pub enum Mood {
    /// Waiting for a password, with this many characters typed.
    Typing(usize),
    /// PAM is thinking.
    Checking,
    /// The last attempt was refused.
    Wrong,
}

/// How wide the indicator is, and how thick its line is, at a scale of 1.
const FIELD: i32 = 440;
const RULE: i32 = 2;
const DOT: i32 = 10;
const DOT_GAP: i32 = 18;
/// Beyond this many characters the dots stop being added, so a long password cannot
/// run off the screen and cannot report its own length.
const MAX_DOTS: usize = 16;

pub fn render(pixels: &mut [u32], width: i32, height: i32, theme: &Theme, mood: Mood) {
    pixels.fill(theme.background);

    // A scale that keeps the indicator the same apparent size on a large display.
    let scale = (width / 960).max(1);
    let field = FIELD * scale;
    let rule = RULE * scale;
    let dot = DOT * scale;
    let gap = DOT_GAP * scale;

    let centre_x = width / 2;
    let centre_y = height / 2;

    let line_colour = match mood {
        Mood::Typing(0) => theme.muted,
        Mood::Typing(_) => theme.accent,
        Mood::Checking => theme.muted,
        Mood::Wrong => theme.error,
    };
    fill(
        pixels,
        width,
        height,
        centre_x - field / 2,
        centre_y + 24 * scale,
        field,
        rule,
        line_colour,
    );

    match mood {
        Mood::Typing(count) => {
            let shown = count.min(MAX_DOTS);
            if shown == 0 {
                return;
            }
            let span = (shown as i32 - 1) * gap;
            let start = centre_x - span / 2 - dot / 2;
            for index in 0..shown {
                fill(
                    pixels,
                    width,
                    height,
                    start + index as i32 * gap,
                    centre_y - 12 * scale,
                    dot,
                    dot,
                    theme.accent,
                );
            }
        }
        // A single mark in the middle while PAM is out, so a slow answer does not
        // look like a lock screen that has stopped responding.
        Mood::Checking => fill(
            pixels,
            width,
            height,
            centre_x - dot / 2,
            centre_y - 12 * scale,
            dot,
            dot,
            theme.muted,
        ),
        // Nothing above the line. The dots are cleared on a refusal, because leaving
        // them says how long the wrong guess was.
        Mood::Wrong => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn fill(
    pixels: &mut [u32],
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    colour: u32,
) {
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(width);
    let y1 = (y + h).min(height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for row in y0..y1 {
        let start = (row * width + x0) as usize;
        let end = (row * width + x1) as usize;
        pixels[start..end].fill(colour);
    }
}
