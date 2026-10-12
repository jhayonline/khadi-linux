//! Makes the pictures for the boot splash: the desktop's name in strips, in the
//! colours of the theme in use, and the blocks of its bar. Plymouth, which draws the
//! screen while the machine boots, can only place whole pictures, so each strip that
//! a glitch moves is a picture of its own.

use crate::theme::Theme;
use std::path::Path;

/// The name, at twice the size it is shown at: two bytes a pixel, how much of it is
/// letter and how much is the hyphen. The compositor's splash is drawn from the same.
const WORDMARK: &[u8] = include_bytes!("../../compositor/assets/wordmark.la");
const WORDMARK_SIZE: (u32, u32) = (642, 126);
pub const STRIPS: u32 = 9;
const TEXT: [u8; 3] = [0xDC, 0xE7, 0xEC];
const COOL: [u8; 3] = [0x4F, 0xD8, 0xCE];
const TRACK: [u8; 3] = [0x1A, 0x23, 0x2B];
const BLOCK: (u32, u32) = (12, 6);

/// The name in one colouring, at the size it is shown at.
fn wordmark(letters: [u8; 3], hyphen: [u8; 3]) -> image::RgbaImage {
    let (width, height) = WORDMARK_SIZE;
    let full = image::RgbaImage::from_fn(width, height, |x, y| {
        let at = ((y * width + x) * 2) as usize;
        let (letter, dash) = (WORDMARK[at] as u32, WORDMARK[at + 1] as u32);
        let alpha = (letter + dash).min(255);
        let mix = |channel: usize| ((letters[channel] as u32 * letter + hyphen[channel] as u32 * dash) / alpha.max(1)).min(255) as u8;
        image::Rgba([mix(0), mix(1), mix(2), alpha as u8])
    });
    image::imageops::resize(&full, width / 2, height / 2, image::imageops::FilterType::Triangle)
}

/// Writes the pictures into `dir`. Returns how many.
pub fn write(dir: &Path, theme: &Theme) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let accent = [theme.main.r(), theme.main.g(), theme.main.b()];
    let save = |image: &image::RgbaImage, name: String| {
        let path = dir.join(name);
        image.save_with_format(&path, image::ImageFormat::Png).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    let mut written = 0;
    for (colouring, (letters, hyphen)) in [(TEXT, accent), (accent, accent), (COOL, COOL)].into_iter().enumerate() {
        let whole = wordmark(letters, hyphen);
        let rows = whole.height() / STRIPS;
        for strip in 0..STRIPS {
            let piece = image::imageops::crop_imm(&whole, 0, strip * rows, whole.width(), rows).to_image();
            save(&piece, format!("strip-{colouring}-{strip}.png"))?;
            written += 1;
        }
    }
    for (name, colour) in [("block-on.png", accent), ("block-off.png", TRACK)] {
        let block = image::RgbaImage::from_pixel(BLOCK.0, BLOCK.1, image::Rgba([colour[0], colour[1], colour[2], 255]));
        save(&block, name.to_string())?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_every_strip_and_both_blocks() {
        assert_eq!(WORDMARK.len() as u32, WORDMARK_SIZE.0 * WORDMARK_SIZE.1 * 2);
        let dir = std::env::temp_dir().join(format!("des-boot-{}", std::process::id()));
        let theme = Theme::load(crate::theme::DEFAULT_NAME);
        assert_eq!(write(&dir, &theme), Ok(29));
        let strip = image::open(dir.join("strip-0-4.png")).unwrap().to_rgba8();
        assert_eq!((strip.width(), strip.height()), (321, 7));
        // The middle strip crosses the letters: some of it is drawn, some is not.
        assert!(strip.pixels().any(|pixel| pixel[3] > 200) && strip.pixels().any(|pixel| pixel[3] == 0));
        let block = image::open(dir.join("block-on.png")).unwrap().to_rgba8();
        assert_eq!(block.get_pixel(0, 0).0, [theme.main.r(), theme.main.g(), theme.main.b(), 255]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
