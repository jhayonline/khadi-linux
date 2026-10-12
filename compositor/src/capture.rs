//! Screenshots and screen recordings.
//!
//! A capture draws a display once more, off screen, and reads the picture back. A
//! screenshot hands it to the shell as a file of plain pixels, for the shell to crop,
//! save and announce; a recording feeds one after another to `ffmpeg`.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::time::{Duration, Instant};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{Bind, ExportMem, Offscreen, damage::OutputDamageTracker, gles::{GlesRenderer, GlesTexture}},
    },
    desktop::space::space_render_elements,
    output::Output,
    utils::{Logical, Rectangle, SERIAL_COUNTER, Transform},
};

use crate::state::DesComp;
use crate::udev::OutputElements;
use des_common::ipc::{Event, ShotMode};

/// A picture of a display: rows from the top, four bytes (red, green, blue, alpha) a pixel.
pub struct Frame {
    pub width: i32,
    pub height: i32,
    pub rgba: Vec<u8>,
}

impl Frame {
    /// The part of the picture inside `area`, given in the picture's own coordinates.
    pub fn crop(&self, area: Rectangle<i32, Logical>) -> Option<Frame> {
        let whole = Rectangle::<i32, Logical>::from_size((self.width, self.height).into());
        let area = whole.intersection(area).filter(|area| area.size.w > 0 && area.size.h > 0)?;
        let mut rgba = Vec::with_capacity((area.size.w * area.size.h * 4) as usize);
        for row in area.loc.y..area.loc.y + area.size.h {
            let start = ((row * self.width + area.loc.x) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + (area.size.w * 4) as usize]);
        }
        Some(Frame { width: area.size.w, height: area.size.h, rgba })
    }
}

/// Recordings aim for this many pictures a second.
const RATE: u32 = 30;

/// A recording under way.
pub struct Recording {
    screen: usize,
    size: (i32, i32),
    /// Pictures on their way to the encoder; dropping this ends the recording.
    frames: SyncSender<Vec<u8>>,
    last: Instant,
    began: Instant,
    /// How many pictures were taken, how many of them the encoder was too slow for,
    /// and how long the taking took in all: said when the recording ends.
    taken: u32,
    let_go: u32,
    spent: Duration,
}

/// Starts the program that turns pictures into a film. `DES_RECORDER` names another
/// program to hand them to instead, which is how this is tried out.
fn encoder(size: (i32, i32), path: &str) -> std::io::Result<Child> {
    let program = std::env::var("DES_RECORDER").unwrap_or_else(|_| "ffmpeg".to_string());
    Command::new(program)
        .args(["-loglevel", "error", "-y", "-f", "rawvideo", "-pixel_format", "rgba"])
        .args(["-video_size", &format!("{}x{}", size.0, size.1)])
        // Pictures come when the screen is drawn, not on a beat: each is stamped with
        // the time it arrived.
        .args(["-use_wallclock_as_timestamps", "1", "-i", "-"])
        // The usual film format wants even sizes.
        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23"])
        .args(["-pix_fmt", "yuv420p", "-fps_mode", "vfr", "-movflags", "+faststart"])
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

impl DesComp {
    fn output_of(&self, screen: usize) -> Option<(Output, Rectangle<i32, Logical>)> {
        let rect = *self.screens.get(screen)?;
        let output = self.space.outputs().find(|output| self.space.output_geometry(output) == Some(rect))?;
        Some((output.clone(), rect))
    }

    /// Draws a display off screen and reads it back, with the pointer if wanted.
    pub fn grab(&mut self, screen: usize, pointer: bool) -> Result<Frame, String> {
        let (output, rect) = self.output_of(screen).ok_or("there is no such display")?;
        let toast_area = self.toast_area();
        let primary = self.screens.first().map(|screen| screen.loc);
        let at = self.seat.get_pointer().map(|pointer| pointer.current_location());
        let scale = output.current_scale().fractional_scale();

        // The renderer belongs to whichever backend is running; it is taken out for
        // the moment so that the rest of the state can be read beside it.
        let mut udev = self.udev.take();
        let mut backend = self.backend.take();
        let mut splash = self.splash.take();
        let result = (|| {
            let mut elements: Vec<OutputElements> = Vec::new();
            let renderer: &mut GlesRenderer = match (&mut udev, &mut backend) {
                (Some(udev), _) => {
                    if let Some(at) = at.filter(|at| pointer && rect.to_f64().contains(*at)) {
                        let position = at - rect.loc.to_f64();
                        elements.extend(crate::udev::cursor_elements(&mut udev.renderer, &udev.cursor, &self.cursor_status, position, scale));
                    }
                    &mut udev.renderer
                }
                (None, Some(backend)) => backend.renderer(),
                (None, None) => return Err("nothing is drawing yet".to_string()),
            };
            // While the splash is up it is all there is to see.
            if let Some(splash) = splash.as_mut() {
                elements.clear();
                elements.extend(crate::udev::splash_elements(splash, renderer, rect.size, scale));
            }
            elements.extend(
                crate::toast::elements(renderer, self.shell.as_ref(), toast_area, primary, &self.toast_id, rect.loc, scale)
                    .into_iter()
                    .map(OutputElements::Toast),
            );
            let space = space_render_elements(renderer, [&self.space], &output, 1.0).map_err(|e| format!("{e:?}"))?;
            elements.extend(space.into_iter().map(OutputElements::Space));

            // The picture is the display as it is looked at: turned the way it stands.
            let size = output
                .current_mode()
                .map(|mode| output.current_transform().transform_size(mode.size))
                .ok_or("the display has no mode")?;
            let mut texture: GlesTexture = renderer
                .create_buffer(Fourcc::Abgr8888, (size.w, size.h).into())
                .map_err(|e| format!("cannot make room for the picture: {e}"))?;
            let mapping = {
                let mut target = renderer.bind(&mut texture).map_err(|e| format!("{e}"))?;
                // Drawn the right way up whatever way the display itself is turned: the
                // windowed backend's is upside down, as far as drawing goes.
                OutputDamageTracker::new((size.w, size.h), scale, Transform::Normal)
                    .render_output(renderer, &mut target, 0, &elements, [0.0, 0.0, 0.0, 1.0])
                    .map_err(|e| format!("cannot draw the picture: {e:?}"))?;
                renderer
                    .copy_framebuffer(&target, Rectangle::from_size((size.w, size.h).into()), Fourcc::Abgr8888)
                    .map_err(|e| format!("cannot read the picture back: {e}"))?
            };
            let rgba = renderer.map_texture(&mapping).map_err(|e| format!("{e}"))?.to_vec();
            Ok(Frame { width: size.w, height: size.h, rgba })
        })();
        self.udev = udev;
        self.backend = backend;
        self.splash = splash;
        result
    }

    /// Takes a screenshot and hands it to the shell: the display in use, the window in
    /// use, or (for the shell to cut from, or to ask what is wanted) the display as it
    /// is now.
    pub fn take_shot(&mut self, mode: ShotMode) {
        let screen = self.focus_screen;
        // Where the window in use lies on its display, if there is one.
        let window = self.focused_on(screen).and_then(|index| {
            let mut area = self.space.element_geometry(&self.apps[index].window)?;
            area.loc -= self.screens[screen].loc;
            Some(area)
        });
        let across = self.screens.get(screen).map_or(1, |screen| screen.size.w.max(1)) as f64;
        let mut window = window;
        let frame = self.grab(screen, false).and_then(|frame| {
            // From places on the desktop to pixels of the picture, which differ on a
            // scaled display.
            let factor = frame.width as f64 / across;
            window = window.map(|area| area.to_f64().upscale(factor).to_i32_round());
            Ok(frame)
        });
        let frame = frame.and_then(|frame| match (mode, window) {
            (ShotMode::Window, Some(area)) => frame.crop(area).ok_or("the window is not on screen".to_string()),
            // Without a window in use there is the whole display.
            _ => Ok(frame),
        });
        let frame = match frame {
            Ok(frame) => frame,
            Err(e) => {
                tracing::warn!("screenshot failed: {e}");
                return;
            }
        };
        self.shots += 1;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let path = runtime.join(format!("des-shot-{}-{}.rgba", std::process::id(), self.shots));
        if let Err(e) = std::fs::write(&path, &frame.rgba) {
            tracing::warn!("cannot hand over the screenshot: {e}");
            return;
        }
        let mut window = window.map(|area| (area.loc.x.max(0), area.loc.y.max(0), area.size.w, area.size.h));
        if matches!(mode, ShotMode::Region | ShotMode::Pick) {
            // The rest happens in the shell's window, which has to be the one in view
            // and the one the keys go to.
            self.focus_screen = 0;
            self.picking = true;
            let shell = self.shell.as_ref().and_then(crate::policy::focus_of);
            if let Some(keyboard) = self.seat.get_keyboard() {
                keyboard.set_focus(self, shell, SERIAL_COUNTER.next_serial());
            }
        } else {
            window = None;
        }
        self.tell(Event::shot(mode, frame.width, frame.height, window, &path.to_string_lossy()));
    }

    /// Starts recording the display in use into `path`, or says why not.
    pub fn start_recording(&mut self, path: &str) {
        if self.recording.is_some() {
            return;
        }
        let screen = self.focus_screen;
        let started = self.grab(screen, true).and_then(|first| {
            let size = (first.width, first.height);
            let mut child = encoder(size, path).map_err(|e| format!("ffmpeg could not be started: {e}"))?;
            let mut stdin = child.stdin.take().ok_or("ffmpeg takes no input")?;
            // Two pictures may wait; more than that and the newest are let go, so that
            // a slow encoder never holds up the screen.
            let (frames, waiting) = sync_channel::<Vec<u8>>(2);
            std::thread::spawn(move || {
                for frame in waiting {
                    if stdin.write_all(&frame).is_err() {
                        break;
                    }
                }
                drop(stdin);
                let _ = child.wait();
            });
            let _ = frames.try_send(first.rgba);
            Ok(Recording { screen, size, frames, last: Instant::now(), began: Instant::now(), taken: 1, let_go: 0, spent: Duration::ZERO })
        });
        match started {
            Ok(recording) => {
                tracing::info!(path, "recording");
                self.recording = Some(recording);
                self.tell_recording("on");
            }
            Err(e) => {
                tracing::warn!("cannot record: {e}");
                self.tell_recording("failed");
            }
        }
    }

    pub fn stop_recording(&mut self) {
        if let Some(recording) = self.recording.take() {
            let seconds = recording.began.elapsed().as_secs_f32();
            let each = recording.spent.as_secs_f32() * 1000.0 / recording.taken.max(1) as f32;
            tracing::info!(seconds, pictures = recording.taken, let_go = recording.let_go, ms_each = each, "recording ended");
            self.tell_recording("off");
        }
    }

    fn tell_recording(&mut self, state: &str) {
        self.tell(Event::recording(state));
    }

    /// Adds a picture to the recording under way, when one is due. Called once a frame.
    pub fn record_tick(&mut self) {
        let Some(recording) = self.recording.as_ref() else {
            return;
        };
        if recording.last.elapsed() < Duration::from_secs(1) / RATE {
            return;
        }
        let (screen, size) = (recording.screen, recording.size);
        let began = Instant::now();
        match self.grab(screen, true) {
            // A display that changed size cannot go on in the same film.
            Ok(frame) if (frame.width, frame.height) == size => {
                let recording = self.recording.as_mut().unwrap();
                recording.last = Instant::now();
                recording.taken += 1;
                recording.spent += began.elapsed();
                // Full or closed: the picture is let go; a closed encoder ends things.
                match recording.frames.try_send(frame.rgba) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::TrySendError::Full(_)) => recording.let_go += 1,
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => self.stop_recording(),
                }
            }
            Ok(_) | Err(_) => self.stop_recording(),
        }
    }
}
