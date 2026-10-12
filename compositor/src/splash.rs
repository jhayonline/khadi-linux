//! The splash: what is on screen while the shell is restarted, in place of a blank
//! screen, until the shell has something to show again.
//!
//! It is the desktop's name, which glitches now and then, over a bar of blocks that
//! fills. The compositor draws it itself, since at that point it is all there is. The
//! boot splash (`session/boot`) is the same screen, drawn by Plymouth.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                Id, Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                render_elements,
                solid::SolidColorRenderElement,
            },
            gles::GlesRenderer,
            utils::CommitCounter,
        },
    },
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
};

render_elements! {
    /// What is drawn above the windows: the splash, and the shell's toasts.
    pub TopElements<=GlesRenderer>;
    Toast=crate::toast::ToastElement,
    Buffer=MemoryRenderBufferRenderElement<GlesRenderer>,
    Solid=SolidColorRenderElement,
}

/// The name, drawn at twice the size it is shown at: two bytes a pixel, how much of
/// it is letter and how much is the hyphen, which takes the accent colour.
const WORDMARK: &[u8] = include_bytes!("../assets/wordmark.la");
const WORDMARK_SIZE: (usize, usize) = (642, 126);
/// The name is cut into this many strips, which a glitch pushes apart.
const STRIPS: usize = 9;

const GROUND: [f32; 3] = [0.024, 0.031, 0.043];
const TEXT: [u8; 3] = [0xDC, 0xE7, 0xEC];
const TRACK: [f32; 3] = [0.102, 0.137, 0.169];
/// The second colour of a glitch, beside the accent.
const COOL: [u8; 3] = [0x4F, 0xD8, 0xCE];
/// The accent of the built-in theme, for when the chosen one cannot be read.
const DEFAULT_ACCENT: [u8; 3] = [79, 227, 216];

const BLOCKS: usize = 20;
const BLOCK: (i32, i32) = (12, 6);
const BLOCK_GAP: i32 = 4;
/// From the bottom of the name to the bar.
const BAR_BELOW: i32 = 44;

/// How long the bar stays full before the splash goes.
const FULL_FOR: Duration = Duration::from_millis(180);
const FADE: Duration = Duration::from_millis(260);
/// When, in each stretch of this length, the name glitches: from, and for how long,
/// in milliseconds.
const GLITCH_EVERY: u128 = 2400;
const GLITCHES: [(u128, u128); 3] = [(320, 130), (1280, 90), (1900, 140)];
/// A glitch changes shape this often, in milliseconds.
const GLITCH_STEP: u128 = 42;

pub struct Splash {
    began: Instant,
    /// The least time it shows, so that it reads as more than a flicker.
    least: Duration,
    /// When the shell had its window.
    ready: Option<Instant>,
    /// The name's strips in its own colours, in the accent, and in the cool colour.
    strips: [Vec<MemoryRenderBuffer>; 3],
    accent: [f32; 3],
    backdrop: Id,
    blocks: Vec<Id>,
    drawn: CommitCounter,
}

/// A number that looks like chance and is the same every time for the same `seed`.
fn chance(seed: u64) -> u64 {
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0xD1B5_4A32_D192_ED03);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 32)
}

/// Which shape of glitch is showing `millis` after the start, if one is.
pub fn glitch_at(millis: u128) -> Option<u64> {
    let within = millis % GLITCH_EVERY;
    GLITCHES
        .iter()
        .enumerate()
        .find(|(_, (from, long))| within >= *from && within < from + long)
        .map(|(which, (from, _))| (millis / GLITCH_EVERY) as u64 * 97 + which as u64 * 13 + ((within - from) / GLITCH_STEP) as u64)
}

/// How many blocks of the bar are filled: quickly at first, then ever slower, never
/// all of them until the shell is `ready`.
pub fn blocks_filled(elapsed: Duration, ready: bool) -> usize {
    if ready {
        return BLOCKS;
    }
    let part = 1.0 - (-elapsed.as_secs_f32() / 0.9).exp();
    ((part * 0.92 * BLOCKS as f32) as usize).min(BLOCKS - 1)
}

/// Reads the accent out of a theme file: the `r`, `g` and `b` of its colours.
pub fn accent_in(theme: &str) -> Option<[u8; 3]> {
    let colors = &theme[theme.find("\"colors\"")?..];
    let colors = &colors[..colors.find('}')?];
    let channel = |name: &str| -> Option<u8> {
        let after = &colors[colors.find(&format!("\"{name}\""))? + name.len() + 2..];
        let digits: String = after.trim_start().strip_prefix(':')?.trim_start().chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    Some([channel("r")?, channel("g")?, channel("b")?])
}

/// The accent of the theme the user chose, as the shell would find it.
fn chosen_accent() -> [u8; 3] {
    let Some(config) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    else {
        return DEFAULT_ACCENT;
    };
    let chosen = std::fs::read_to_string(config.join("des-ui/config")).ok().and_then(|text| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "theme").then(|| value.trim().to_string())
        })
    });
    let Some(name) = chosen.filter(|name| !name.is_empty() && !name.contains('/') && !name.contains("..")) else {
        return DEFAULT_ACCENT;
    };
    [config.join("des-ui/themes"), PathBuf::from("/usr/local/share/des-ui/themes"), PathBuf::from("/usr/share/des-ui/themes")]
        .iter()
        .find_map(|dir| accent_in(&std::fs::read_to_string(dir.join(format!("{name}.json"))).ok()?))
        .unwrap_or(DEFAULT_ACCENT)
}

/// The name's strips in one colouring: letters in `letters`, the hyphen in `hyphen`.
fn strips(letters: [u8; 3], hyphen: [u8; 3]) -> Vec<MemoryRenderBuffer> {
    let (width, height) = WORDMARK_SIZE;
    let rows = height / STRIPS;
    (0..STRIPS)
        .map(|strip| {
            let from = strip * rows;
            // The last strip takes the rows left over.
            let to = if strip + 1 == STRIPS { height } else { from + rows };
            let mut data = Vec::with_capacity(width * (to - from) * 4);
            for pixel in WORDMARK[from * width * 2..to * width * 2].chunks_exact(2) {
                let (letter, dash) = (pixel[0] as u32, pixel[1] as u32);
                let alpha = (letter + dash).min(255);
                // Stored as blue, green, red, alpha, each colour already scaled by the alpha.
                for channel in [2, 1, 0] {
                    data.push(((letters[channel] as u32 * letter + hyphen[channel] as u32 * dash) / 255).min(255) as u8);
                }
                data.push(alpha as u8);
            }
            MemoryRenderBuffer::from_slice(&data, Fourcc::Argb8888, (width as i32, (to - from) as i32), 2, Transform::Normal, None)
        })
        .collect()
}

impl Splash {
    pub fn new(least: Duration) -> Splash {
        let accent = chosen_accent();
        Splash {
            began: Instant::now(),
            least,
            ready: None,
            strips: [strips(TEXT, accent), strips(accent, accent), strips(COOL, COOL)],
            accent: accent.map(|channel| channel as f32 / 255.0),
            backdrop: Id::new(),
            blocks: (0..BLOCKS).map(|_| Id::new()).collect(),
            drawn: CommitCounter::default(),
        }
    }

    /// The shell has its window: the bar fills and the splash makes way.
    pub fn shell_ready(&mut self) {
        self.ready.get_or_insert_with(Instant::now);
    }

    /// When the bar is full, which is when the splash starts to go.
    fn full_at(&self) -> Option<Instant> {
        Some(self.ready?.max(self.began + self.least))
    }

    /// Whether there is nothing left of it to show.
    pub fn done(&self) -> bool {
        self.full_at().is_some_and(|at| at.elapsed() >= FULL_FOR + FADE)
    }

    /// How much of it shows: all, until it fades at the end.
    fn strength(&self) -> f32 {
        match self.full_at().map(|at| at.elapsed()) {
            Some(since) if since > FULL_FOR => 1.0 - ((since - FULL_FOR).as_secs_f32() / FADE.as_secs_f32()).min(1.0),
            _ => 1.0,
        }
    }

    /// The splash as something to draw on a display of this size, which is drawn at
    /// `scale`. Front to back.
    pub fn elements(&mut self, renderer: &mut GlesRenderer, size: Size<i32, Logical>, scale: f64) -> Vec<TopElements> {
        self.drawn.increment();
        let strength = self.strength();
        let elapsed = self.began.elapsed();
        let physical = |x: f64, y: f64| -> Point<f64, Physical> { Point::<f64, Logical>::from((x, y)).to_physical(scale) };
        let mut elements = Vec::new();

        let (name_w, name_h) = (WORDMARK_SIZE.0 as f64 / 2.0, WORDMARK_SIZE.1 as f64 / 2.0);
        let bar_w = (BLOCKS as i32 * (BLOCK.0 + BLOCK_GAP) - BLOCK_GAP) as f64;
        let whole_h = name_h + BAR_BELOW as f64 + BLOCK.1 as f64;
        let left = (size.w as f64 - name_w) / 2.0;
        let top = (size.h as f64 - whole_h) / 2.0;

        // The name, a strip at a time. In a glitch some strips slide aside, and a few
        // show again in colour, slid the other way.
        let glitch = glitch_at(elapsed.as_millis());
        // Strips are whole rows of the picture, which is drawn at half its size.
        let rows = (WORDMARK_SIZE.1 / STRIPS) as f64 / 2.0;
        for strip in 0..STRIPS {
            let y = top + strip as f64 * rows;
            let (mut slide, mut ghosts) = (0.0, [None, None]);
            if let Some(shape) = glitch {
                let roll = chance(shape * 31 + strip as u64);
                // The whole name jumps a little; about half the strips jump further.
                slide = (chance(shape) % 9) as f64 - 4.0;
                if roll % 2 == 0 {
                    slide += (roll >> 8) as f64 % 25.0 - 12.0;
                }
                if roll % 3 == 0 {
                    ghosts[0] = Some((roll >> 16) as f64 % 15.0 + 4.0);
                }
                if roll % 4 == 1 {
                    ghosts[1] = Some(-((roll >> 24) as f64 % 15.0 + 4.0));
                }
            }
            let mut put = |colouring: usize, x: f64| {
                let buffer = &self.strips[colouring][strip];
                if let Ok(element) = MemoryRenderBufferRenderElement::from_buffer(renderer, physical(x, y), buffer, Some(strength), None, None, Kind::Unspecified) {
                    elements.push(TopElements::Buffer(element));
                }
            };
            put(0, left + slide);
            for (colouring, ghost) in ghosts.into_iter().enumerate() {
                if let Some(aside) = ghost {
                    put(colouring + 1, left + slide + aside);
                }
            }
        }

        // The bar: a block for each step, the filled ones in the accent.
        let filled = blocks_filled(elapsed, self.full_at().is_some_and(|at| at <= Instant::now()));
        let bar_left = (size.w as f64 - bar_w) / 2.0;
        let bar_top = top + name_h + BAR_BELOW as f64;
        let solid = |id: &Id, x: f64, y: f64, w: f64, h: f64, colour: [f32; 3], commit: CommitCounter| {
            let area: Rectangle<i32, Physical> = Rectangle::<f64, Logical>::new((x, y).into(), (w, h).into()).to_physical(scale).to_i32_round();
            let colour = [colour[0] * strength, colour[1] * strength, colour[2] * strength, strength];
            TopElements::Solid(SolidColorRenderElement::new(id.clone(), area, commit, colour, Kind::Unspecified))
        };
        for (index, id) in self.blocks.iter().enumerate() {
            let x = bar_left + (index as i32 * (BLOCK.0 + BLOCK_GAP)) as f64;
            let colour = if index < filled { self.accent } else { TRACK };
            elements.push(solid(id, x, bar_top, BLOCK.0 as f64, BLOCK.1 as f64, colour, self.drawn));
        }

        // Behind it all, the ground, over whatever else there is.
        elements.push(solid(&self.backdrop, 0.0, 0.0, size.w as f64, size.h as f64, GROUND, self.drawn));
        elements
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_still_between_glitches() {
        assert_eq!(glitch_at(0), None);
        assert!(glitch_at(330).is_some());
        assert_eq!(glitch_at(600), None);
        // A glitch changes shape as it goes, and is not the same the next time round.
        assert_ne!(glitch_at(330), glitch_at(330 + 2 * GLITCH_STEP));
        assert_ne!(glitch_at(330), glitch_at(330 + GLITCH_EVERY));
        let glitching = (0..GLITCH_EVERY).filter(|millis| glitch_at(*millis).is_some()).count();
        assert_eq!(glitching, 360, "still for most of the time");
    }

    #[test]
    fn the_bar_only_fills_when_the_shell_is_there() {
        assert_eq!(blocks_filled(Duration::ZERO, false), 0);
        let (early, late) = (blocks_filled(Duration::from_millis(400), false), blocks_filled(Duration::from_secs(3), false));
        assert!(early > 0 && early < late && late < BLOCKS);
        assert_eq!(blocks_filled(Duration::from_secs(600), false), BLOCKS - 2, "waiting, however long, is not done");
        assert_eq!(blocks_filled(Duration::from_millis(100), true), BLOCKS);
    }

    #[test]
    fn reads_a_theme_s_accent() {
        let theme = r##"{"name": "blade", "colors": {"r": 204, "g":94 , "b": 55, "black": "#000000"}, "terminal": {"r": 1}}"##;
        assert_eq!(accent_in(theme), Some([204, 94, 55]));
        assert_eq!(accent_in(r#"{"colors": {"r": 1, "g": 2}}"#), None);
        assert_eq!(accent_in(r#"{"colors": {"r": 999, "g": 2, "b": 3}}"#), None);
        assert_eq!(accent_in("{}"), None);
    }

    #[test]
    fn the_name_s_picture_is_the_size_it_says() {
        assert_eq!(WORDMARK.len(), WORDMARK_SIZE.0 * WORDMARK_SIZE.1 * 2);
        let all: usize = strips(TEXT, DEFAULT_ACCENT).len();
        assert_eq!(all, STRIPS);
    }
}
