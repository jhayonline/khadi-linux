//! Screenshots and screen recordings, as far as the shell is concerned.
//!
//! The compositor takes the pictures. The shell cuts a piece out when one is asked
//! for, saves screenshots as PNG files, offers them to the clipboard, names the files
//! recordings go into, and says what became of each.

use crate::notify::Center;
use crate::ui;
use khadi_common::ipc::{Command, Recording, ShotMode};
use eframe::egui::{self, Align2, Area, Color32, Id, Key, Order, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, pos2, vec2};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

/// A picture as the compositor hands it over: rows from the top, four bytes a pixel.
#[derive(Clone)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Picture {
    /// The part inside the rectangle from (`x`, `y`), clipped to the picture.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Option<Picture> {
        let x = x.min(self.width);
        let y = y.min(self.height);
        let width = width.min(self.width - x);
        let height = height.min(self.height - y);
        if width == 0 || height == 0 {
            return None;
        }
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for row in y..y + height {
            let start = ((row * self.width + x) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + (width * 4) as usize]);
        }
        Some(Picture { width, height, rgba })
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// A path as it is said to the user: with `~` for their home.
pub fn spoken(path: &Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn stamped(folder: &str, subfolder: &str, name: &str, extension: &str) -> std::io::Result<PathBuf> {
    let dir = home().join(folder).join(subfolder);
    std::fs::create_dir_all(&dir)?;
    let when = chrono::Local::now().format("%Y-%m-%d %H-%M-%S");
    Ok(dir.join(format!("{name} {when}.{extension}")))
}

/// Saves a picture and offers it to the clipboard. Returns where, and whether it was
/// also copied.
fn save(picture: Picture) -> Result<(PathBuf, bool), String> {
    let path = stamped("Pictures", "Screenshots", "Screenshot", "png").map_err(|e| format!("cannot make the folder: {e}"))?;
    let mut rgba = picture.rgba;
    // What is on screen is not see-through, whatever the fourth byte says.
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    let image = image::RgbaImage::from_raw(picture.width, picture.height, rgba).ok_or("the picture is not the size it claims")?;
    image.save(&path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    // `wl-copy` keeps the picture on offer after this returns; without it there is
    // only the file.
    let copied = std::fs::File::open(&path)
        .and_then(|file| {
            std::process::Command::new("wl-copy")
                .args(["--type", "image/png"])
                .stdin(file)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
        })
        .is_ok_and(|status| status.success());
    Ok((path, copied))
}

/// A screenshot as the compositor announces it.
pub struct Shot {
    pub mode: ShotMode,
    pub width: u32,
    pub height: u32,
    /// Where in it the window in use lies: left, top, width, height.
    pub window: Option<(u32, u32, u32, u32)>,
    pub path: String,
}

/// A still of the display, shown while the user says what of it to keep.
struct Marking {
    picture: Picture,
    texture: Option<TextureHandle>,
    /// Where the window in use lies in the picture.
    window: Option<(u32, u32, u32, u32)>,
    /// Still being asked what to take, as opposed to marking out a piece.
    choosing: bool,
    from: Option<Pos2>,
}

/// What the user settled on.
enum Choice {
    Screen,
    Window,
    Piece(Rect),
    Record,
    Cancel,
}

pub struct Capture {
    marking: Option<Marking>,
    saved: (Sender<Result<(PathBuf, bool), String>>, Receiver<Result<(PathBuf, bool), String>>),
    /// When the recording under way began.
    pub recording: Option<Instant>,
    /// The file the recording is going into, or went into.
    film: Option<PathBuf>,
    ctx: egui::Context,
}

impl Capture {
    pub fn new(ctx: egui::Context) -> Capture {
        Capture { marking: None, saved: channel(), recording: None, film: None, ctx }
    }

    /// Takes over a screenshot the compositor took.
    pub fn take(&mut self, shot: Shot) {
        let read = std::fs::read(&shot.path);
        let _ = std::fs::remove_file(&shot.path);
        // The key pressed again while asking calls it off.
        if shot.mode == ShotMode::Pick && self.marking.take().is_some() {
            return;
        }
        let Ok(rgba) = read else { return };
        if rgba.len() != (shot.width as usize) * (shot.height as usize) * 4 || rgba.is_empty() {
            return;
        }
        let picture = Picture { width: shot.width, height: shot.height, rgba };
        match shot.mode {
            ShotMode::Pick | ShotMode::Region => {
                let choosing = shot.mode == ShotMode::Pick;
                self.marking = Some(Marking { picture, texture: None, window: shot.window, choosing, from: None });
            }
            ShotMode::Screen | ShotMode::Window => self.keep(picture),
        }
    }

    fn keep(&self, picture: Picture) {
        let (results, ctx) = (self.saved.0.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = results.send(save(picture));
            ctx.request_repaint();
        });
    }

    /// Whether a piece is being marked out, which takes the whole display.
    pub fn marking(&self) -> bool {
        self.marking.is_some()
    }

    pub fn cancel(&mut self) {
        self.marking = None;
    }

    /// Says what became of screenshots saved since last asked.
    pub fn report(&mut self, notices: &mut Center) {
        while let Ok(result) = self.saved.1.try_recv() {
            match result {
                Ok((path, true)) => notices.post("Screenshot saved", &format!("{}\nAlso copied, ready to paste.", spoken(&path)), false),
                Ok((path, false)) => notices.post("Screenshot saved", &spoken(&path), false),
                Err(why) => notices.post("Screenshot not saved", &why, true),
            }
        }
    }

    /// Starts a recording, or ends the one under way. Returns what to tell the compositor.
    pub fn toggle_recording(&mut self, notices: &mut Center) -> Option<Command> {
        if self.recording.is_some() {
            return Some(Command::RecordStop);
        }
        match stamped("Videos", "Recordings", "Recording", "mp4") {
            Ok(path) => {
                let command = Command::Record(path.to_string_lossy().into_owned());
                self.film = Some(path);
                Some(command)
            }
            Err(e) => {
                notices.post("Cannot record", &format!("The folder for recordings could not be made: {e}"), true);
                None
            }
        }
    }

    /// Takes in what the compositor says about the recording.
    pub fn recording_is(&mut self, state: Recording, notices: &mut Center) {
        match state {
            Recording::On => self.recording = Some(Instant::now()),
            Recording::Off => {
                self.recording = None;
                if let Some(path) = self.film.take() {
                    notices.post("Recording saved", &spoken(&path), false);
                }
            }
            Recording::Failed => {
                self.recording = None;
                self.film = None;
                notices.post("Cannot record", "Recording needs ffmpeg. Install it with: sudo apt install ffmpeg", true);
            }
        }
    }

    /// Shows the still and asks what of it to keep: all of it, the window in use, or
    /// a piece marked out by dragging. A click beside the choices, the other button or
    /// Escape calls it off. Returns whether a recording was asked for instead.
    pub fn mark(&mut self, ctx: &egui::Context, accent: Color32) -> bool {
        let recording = self.recording.is_some();
        let Some(marking) = self.marking.as_mut() else {
            return false;
        };
        let screen = ctx.screen_rect();
        let texture = marking.texture.get_or_insert_with(|| {
            let size = [marking.picture.width as usize, marking.picture.height as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &marking.picture.rgba);
            ctx.load_texture("screenshot-still", image, egui::TextureOptions::NEAREST)
        });
        // From pixels of the picture to points on screen, and back.
        let scale = marking.picture.width as f32 / screen.width().max(1.0);
        let window = marking.window.map(|(x, y, width, height)| {
            Rect::from_min_size(screen.min + vec2(x as f32, y as f32) / scale, vec2(width as f32, height as f32) / scale)
        });
        let mut done: Option<Choice> = None;
        Area::new(Id::new("screenshot-marking")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), rect, whole, Color32::WHITE);
            let pointer = response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
            if response.drag_started() {
                marking.from = pointer;
            }
            let chosen = match (marking.from, pointer) {
                (Some(from), Some(to)) => Some(Rect::from_two_pos(from, to).intersect(rect)),
                _ => None,
            };
            // Everything outside what would be kept is dimmed, so that it stands out
            // as it will be.
            let dim = Color32::from_black_alpha(140);
            let spotlight = |piece: Rect| {
                for shade in [
                    Rect::from_min_max(rect.min, pos2(rect.right(), piece.top())),
                    Rect::from_min_max(pos2(rect.left(), piece.bottom()), rect.max),
                    Rect::from_min_max(pos2(rect.left(), piece.top()), pos2(piece.left(), piece.bottom())),
                    Rect::from_min_max(pos2(piece.right(), piece.top()), pos2(rect.right(), piece.bottom())),
                ] {
                    painter.rect_filled(shade, 0.0, dim);
                }
                painter.rect_stroke(piece, 0.0, Stroke::new(1.0, accent), StrokeKind::Outside);
                ui::corners(&painter, piece, 10.0, Stroke::new(2.0, accent));
            };
            match chosen {
                Some(piece) => {
                    spotlight(piece);
                    let size = format!("{} × {}", (piece.width() * scale).round(), (piece.height() * scale).round());
                    painter.text(piece.left_top() + vec2(0.0, -6.0), Align2::LEFT_BOTTOM, size, ui::mono(11.0), accent);
                }
                None if marking.choosing => {
                    let record = if recording { "STOP RECORDING" } else { "RECORD" };
                    let choices = [
                        ("FULL SCREEN", "F", Some(Choice::Screen)),
                        ("AREA", "A", None),
                        ("WINDOW", "W", window.map(|_| Choice::Window)),
                        (record, "R", Some(Choice::Record)),
                    ];
                    let (width, height, gap) = (150.0, 64.0, 10.0);
                    let count = choices.len() as f32;
                    let plate = Rect::from_center_size(
                        pos2(rect.center().x, rect.bottom() - 120.0),
                        vec2(count * width + (count + 1.0) * gap, height + 2.0 * gap),
                    );
                    let mut buttons = Vec::new();
                    let mut hovered = None;
                    for (index, (name, key, choice)) in choices.into_iter().enumerate() {
                        let button = Rect::from_min_size(plate.min + vec2(gap + index as f32 * (width + gap), gap), vec2(width, height));
                        let offered = index == 1 || choice.is_some();
                        let sense = if offered { Sense::click() } else { Sense::hover() };
                        let response = ui.interact(button, Id::new(("screenshot-choice", index)), sense);
                        if response.hovered() && offered {
                            hovered = Some(index);
                        }
                        if response.clicked() {
                            match choice {
                                Some(choice) => done = Some(choice),
                                None => marking.choosing = false,
                            }
                        }
                        buttons.push((button, name, key, offered, response.hovered()));
                    }
                    // What each choice would keep shows behind the choices.
                    match (hovered, window) {
                        (Some(0), _) => spotlight(rect.shrink(1.0)),
                        (Some(2), Some(window)) => spotlight(window.intersect(rect)),
                        _ => {
                            painter.rect_filled(rect, 0.0, dim);
                        }
                    }
                    painter.rect_filled(plate, 0.0, ui::PANEL);
                    painter.rect_stroke(plate, 0.0, Stroke::new(1.0, accent), StrokeKind::Inside);
                    for (button, name, key, offered, over) in buttons {
                        let (ink, edge) = match (offered, over) {
                            (false, _) => (ui::LINE_STRONG, ui::LINE),
                            (true, true) => (accent, accent),
                            (true, false) => (ui::TEXT, ui::LINE_STRONG),
                        };
                        if offered && over {
                            painter.rect_filled(button, 0.0, accent.gamma_multiply(0.12));
                        }
                        painter.rect_stroke(button, 0.0, Stroke::new(1.0, edge), StrokeKind::Inside);
                        painter.text(button.center() + vec2(0.0, -8.0), Align2::CENTER_CENTER, name, ui::display_bold(12.0), ink);
                        painter.text(button.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, key, ui::mono(10.0), if offered { ui::LABEL } else { ui::LINE_STRONG });
                    }
                    let hint = "OR DRAG OVER THE PIECE YOU WANT  ·  ESC TO CANCEL";
                    painter.text(plate.center_bottom() + vec2(0.0, 14.0), Align2::CENTER_TOP, hint, ui::mono(10.0), ui::SECONDARY);
                    ui.input(|i| {
                        if i.key_pressed(Key::F) || i.key_pressed(Key::Enter) {
                            done = Some(Choice::Screen);
                        } else if i.key_pressed(Key::A) {
                            marking.choosing = false;
                        } else if i.key_pressed(Key::W) && window.is_some() {
                            done = Some(Choice::Window);
                        } else if i.key_pressed(Key::R) {
                            done = Some(Choice::Record);
                        }
                    });
                }
                None => {
                    painter.rect_filled(rect, 0.0, dim);
                    let hint = "DRAG TO MARK OUT A PIECE  ·  CLICK OR ESC TO CANCEL";
                    let galley = ui::galley(ui, hint, ui::display_bold(13.0), ui::TEXT, 2.0);
                    let plate = Rect::from_center_size(rect.center_top() + vec2(0.0, 60.0), galley.size() + vec2(36.0, 22.0));
                    painter.rect_filled(plate, 0.0, ui::PANEL);
                    painter.rect_stroke(plate, 0.0, Stroke::new(1.0, accent), StrokeKind::Inside);
                    painter.galley(plate.center() - galley.size() / 2.0, galley, ui::TEXT);
                    if let Some(at) = pointer {
                        painter.hline(rect.x_range(), at.y, Stroke::new(1.0, accent.gamma_multiply(0.5)));
                        painter.vline(at.x, rect.y_range(), Stroke::new(1.0, accent.gamma_multiply(0.5)));
                    }
                }
            }
            if response.drag_stopped() {
                marking.from = None;
                done = Some(match chosen.filter(|piece| piece.width() >= 4.0 && piece.height() >= 4.0) {
                    Some(piece) => Choice::Piece(piece),
                    None => Choice::Cancel,
                });
            } else if response.clicked() || response.secondary_clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                done = Some(Choice::Cancel);
            }
        });
        ctx.request_repaint();
        let Some(choice) = done else {
            return false;
        };
        let marking = self.marking.take().unwrap();
        let picture = marking.picture;
        let at = |value: f32| (value * scale).round().max(0.0) as u32;
        let kept = match choice {
            Choice::Screen => Some(picture),
            Choice::Window => marking.window.and_then(|(x, y, width, height)| picture.crop(x, y, width, height)),
            Choice::Piece(piece) => {
                picture.crop(at(piece.left() - screen.left()), at(piece.top() - screen.top()), at(piece.width()), at(piece.height()))
            }
            Choice::Record => return true,
            Choice::Cancel => None,
        };
        if let Some(kept) = kept {
            self.keep(kept);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuts_a_piece_and_keeps_inside_the_picture() {
        // Three by two, each pixel's red byte its number.
        let rgba: Vec<u8> = (0..6u8).flat_map(|n| [n, 0, 0, 255]).collect();
        let picture = Picture { width: 3, height: 2, rgba };
        let reds = |picture: &Picture| picture.rgba.chunks_exact(4).map(|pixel| pixel[0]).collect::<Vec<_>>();
        let cut = picture.crop(1, 0, 2, 2).unwrap();
        assert_eq!((cut.width, cut.height), (2, 2));
        assert_eq!(reds(&cut), [1, 2, 4, 5]);
        let clipped = picture.crop(2, 1, 10, 10).unwrap();
        assert_eq!(reds(&clipped), [5]);
        assert!(picture.crop(3, 0, 1, 1).is_none());
        assert!(picture.crop(0, 0, 0, 5).is_none());
    }

    #[test]
    fn says_paths_from_home() {
        let inside = home().join("Pictures/Screenshots/a.png");
        assert_eq!(spoken(&inside), "~/Pictures/Screenshots/a.png");
        assert_eq!(spoken(Path::new("/var/tmp/a.png")), "/var/tmp/a.png");
    }
}
