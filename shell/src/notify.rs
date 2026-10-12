//! Notifications: what applications and the desktop itself have to say.
//!
//! The shell is the session's notification service (`org.freedesktop.Notifications`
//! on D-Bus). What arrives is shown for a moment as a toast at the top right of the
//! workspace, above whatever is open, and kept in a list that the status strip opens.
//! Each comes with its sender's picture where one can be found, and with a sound.

use crate::sysmon::Battery;
use crate::theme::Theme;
use crate::ui;
use eframe::egui::{
    self, Align, Area, Id, Key, Layout, Modifiers, Order, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, pos2, vec2,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::zvariant::OwnedValue;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

/// How long a toast stays when its sender does not say.
const STAY: Duration = Duration::from_secs(6);
/// How many toasts are on screen at once; the rest wait their turn.
const AT_ONCE: usize = 3;
/// How many notifications are remembered.
const KEPT: usize = 100;
const TOAST_WIDTH: f32 = 360.0;

/// Why a notification is gone, as its sender is told.
const DISMISSED: u32 = 2;
const WITHDRAWN: u32 = 3;

/// Where the sounds every desktop is expected to have are kept.
const SOUNDS: &str = "/usr/share/sounds/freedesktop/stereo";
/// The least time between two sounds: a burst of notifications is one interruption.
const SOUND_GAP: Duration = Duration::from_millis(1500);
/// Pictures are kept no larger than this, a side.
const ICON_SIDE: u32 = 96;

/// A notification's picture.
pub struct Icon {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
    /// What it is drawn from, made the first time it is drawn.
    texture: Mutex<Option<egui::TextureHandle>>,
}

impl PartialEq for Icon {
    fn eq(&self, other: &Icon) -> bool {
        (self.width, self.height) == (other.width, other.height) && self.rgba == other.rgba
    }
}

impl std::fmt::Debug for Icon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Icon({}x{})", self.width, self.height)
    }
}

impl Icon {
    fn new(image: image::DynamicImage) -> Icon {
        let image = if image.width() > ICON_SIDE || image.height() > ICON_SIDE { image.thumbnail(ICON_SIDE, ICON_SIDE) } else { image };
        let image = image.to_rgba8();
        Icon { width: image.width() as usize, height: image.height() as usize, rgba: image.into_raw(), texture: Mutex::new(None) }
    }

    /// From pixels sent along with the notification: rows of `stride` bytes, three or
    /// four bytes a pixel.
    pub fn from_raw(width: i32, height: i32, stride: i32, channels: i32, data: &[u8]) -> Option<Icon> {
        let (width, height, stride, channels) = (usize::try_from(width).ok()?, usize::try_from(height).ok()?, usize::try_from(stride).ok()?, usize::try_from(channels).ok()?);
        if width == 0 || height == 0 || width > 4096 || height > 4096 || !(3..=4).contains(&channels) || stride < width * channels {
            return None;
        }
        // The last row may stop where its pixels do.
        if data.len() < stride * (height - 1) + width * channels {
            return None;
        }
        let mut rgba = Vec::with_capacity(width * height * 4);
        for row in 0..height {
            for pixel in data[row * stride..row * stride + width * channels].chunks_exact(channels) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], if channels == 4 { pixel[3] } else { 255 }]);
            }
        }
        image::RgbaImage::from_raw(width as u32, height as u32, rgba).map(|image| Icon::new(image.into()))
    }

    fn open(path: &Path) -> Option<Icon> {
        let image = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
        Some(Icon::new(image))
    }

    fn texture(&self, ctx: &egui::Context) -> egui::TextureId {
        self.texture
            .lock()
            .unwrap()
            .get_or_insert_with(|| {
                let image = egui::ColorImage::from_rgba_unmultiplied([self.width, self.height], &self.rgba);
                ctx.load_texture("notice-icon", image, egui::TextureOptions::LINEAR)
            })
            .id()
    }

    /// Draws it in a square of `side`, keeping its shape.
    fn show(&self, ui: &mut Ui, side: f32) {
        let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
        let scale = side / self.width.max(self.height).max(1) as f32;
        let fitted = Rect::from_center_size(rect.center(), vec2(self.width as f32, self.height as f32) * scale);
        let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        ui.painter().image(self.texture(ui.ctx()), fitted, whole, egui::Color32::WHITE);
    }
}

/// The folders icons are installed under, most specific first.
fn icon_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".local/share/icons"));
        roots.push(home.join(".local/share/flatpak/exports/share/icons"));
    }
    roots.extend(["/usr/share/icons", "/usr/local/share/icons", "/var/lib/flatpak/exports/share/icons", "/var/lib/snapd/desktop/icons"].map(PathBuf::from));
    roots
}

/// Finds the file for an icon given by name, among `roots`. Only pictures this shell
/// can read are looked for, which leaves out drawings (SVG).
pub fn find_icon(name: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains("..") {
        return None;
    }
    for root in roots {
        for theme in ["hicolor", "Yaru", "Adwaita"] {
            for size in ["64x64", "48x48", "96x96", "128x128", "256x256", "32x32"] {
                for kind in ["apps", "status", "devices", "actions", "categories", "places", "legacy", "emblems", "mimetypes"] {
                    // Themes disagree on which of the two comes first.
                    for path in [root.join(theme).join(size).join(kind), root.join(theme).join(kind).join(size)] {
                        let file = path.join(format!("{name}.png"));
                        if file.is_file() {
                            return Some(file);
                        }
                    }
                }
            }
        }
    }
    let pixmap = Path::new("/usr/share/pixmaps").join(format!("{name}.png"));
    pixmap.is_file().then_some(pixmap)
}

/// The picture named by a notification: a file, a `file://` address, or an icon's name.
fn named_icon(name: &str) -> Option<Icon> {
    let name = name.trim();
    if let Some(path) = name.strip_prefix("file://") {
        return Icon::open(Path::new(path));
    }
    if name.starts_with('/') {
        return Icon::open(Path::new(name));
    }
    Icon::open(&find_icon(name, &icon_roots())?)
}

/// The sound a notification asks for, by what its hints say: none (`Some(None)`), a
/// particular file, or nothing in particular (`None`).
pub fn asked_sound(suppress: bool, file: Option<&str>, name: Option<&str>) -> Option<Option<PathBuf>> {
    if suppress {
        return Some(None);
    }
    if let Some(file) = file.filter(|file| file.starts_with('/')) {
        return Some(Some(PathBuf::from(file)));
    }
    let name = name.filter(|name| !name.is_empty() && !name.contains('/') && !name.contains(".."))?;
    Some(Some(Path::new(SOUNDS).join(format!("{name}.oga"))))
}

/// Plays a sound file, without waiting for it.
fn play(path: &Path) {
    if !path.is_file() {
        return;
    }
    let started = std::process::Command::new("pw-play")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut child) = started {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub id: u32,
    pub app: String,
    pub title: String,
    pub body: String,
    /// What can be done about it: a key for the sender, and a label for the user.
    pub actions: Vec<(String, String)>,
    pub urgent: bool,
    pub icon: Option<Arc<Icon>>,
    /// The sound it asked for: none, or a file. `None` leaves it to the desktop.
    sound: Option<Option<PathBuf>>,
    /// The time of day it arrived, as shown in the list.
    pub clock: String,
    /// How long its toast stays; `None` until dismissed.
    stay: Option<Duration>,
    /// When its toast went up, while it is up.
    shown: Option<Instant>,
    /// Still waiting for, or showing, its toast.
    fresh: bool,
    /// Sent by an application over D-Bus, which then wants to hear what became of it.
    foreign: bool,
}

impl Notice {
    /// What clicking the notification itself does, if its sender offered that.
    fn default_action(&self) -> Option<&str> {
        self.actions.iter().find(|(key, _)| key == "default").map(|(key, _)| key.as_str())
    }
}

/// Turns the little markup a notification's text may carry into plain text.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        match rest[start..].find('>') {
            Some(end) => {
                let tag = rest[start + 1..start + end].trim().to_ascii_lowercase();
                if tag.starts_with("br") {
                    out.push('\n');
                }
                rest = &rest[start + end + 1..];
            }
            // A lone angle bracket is just text.
            None => {
                out.push('<');
                rest = &rest[start + 1..];
            }
        }
    }
    out.push_str(rest);
    for (entity, character) in [("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&apos;", "'"), ("&amp;", "&")] {
        out = out.replace(entity, character);
    }
    out.trim().to_string()
}

/// Pairs up the flat list of actions D-Bus carries: key, label, key, label…
pub fn pair_actions(flat: &[String]) -> Vec<(String, String)> {
    flat.chunks_exact(2).map(|pair| (pair[0].clone(), pair[1].clone())).collect()
}

fn clock_now() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

/// What the D-Bus side hands over to the interface.
#[derive(Default)]
struct Inbox {
    last_id: u32,
    arrived: Vec<(Notice, u32)>,
    withdrawn: Vec<u32>,
}

struct Service {
    inbox: Arc<Mutex<Inbox>>,
    ctx: egui::Context,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Service {
    fn get_capabilities(&self) -> Vec<String> {
        ["body", "actions", "persistence", "icon-static", "sound"].map(str::to_string).to_vec()
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let urgent = hints.get("urgency").and_then(|value| value.downcast_ref::<u8>().ok()) == Some(2);
        let word = |key: &str| hints.get(key).and_then(|value| value.downcast_ref::<&str>().ok()).filter(|word| !word.is_empty());
        let pixels = |key: &str| {
            let value = hints.get(key)?.try_clone().ok()?;
            let (width, height, stride, _alpha, _bits, channels, data): (i32, i32, i32, bool, i32, i32, Vec<u8>) = value.try_into().ok()?;
            Icon::from_raw(width, height, stride, channels, &data)
        };
        // In the order the specification gives them weight.
        let icon = pixels("image-data")
            .or_else(|| pixels("image_data"))
            .or_else(|| word("image-path").or(word("image_path")).and_then(named_icon))
            .or_else(|| named_icon(&app_icon))
            .or_else(|| pixels("icon_data"))
            .or_else(|| word("desktop-entry").and_then(named_icon));
        let suppress = hints.get("suppress-sound").and_then(|value| value.downcast_ref::<bool>().ok()) == Some(true);
        let sound = asked_sound(suppress, word("sound-file"), word("sound-name"));
        let mut inbox = self.inbox.lock().unwrap();
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            inbox.last_id = inbox.last_id.wrapping_add(1).max(1);
            inbox.last_id
        };
        let notice = Notice {
            id,
            app: plain(&app_name),
            title: plain(&summary),
            body: plain(&body),
            actions: pair_actions(&actions),
            urgent,
            icon: icon.map(Arc::new),
            sound,
            clock: clock_now(),
            stay: match expire_timeout {
                // Urgent ones stay until seen to.
                _ if urgent => None,
                millis if millis > 0 => Some(Duration::from_millis(millis as u64).clamp(Duration::from_secs(2), Duration::from_secs(30))),
                _ => Some(STAY),
            },
            shown: None,
            fresh: true,
            foreign: true,
        };
        inbox.arrived.push((notice, replaces_id));
        drop(inbox);
        self.ctx.request_repaint();
        id
    }

    fn close_notification(&self, id: u32) {
        self.inbox.lock().unwrap().withdrawn.push(id);
        self.ctx.request_repaint();
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("DEs-UI".to_string(), "DEs-UI".to_string(), env!("CARGO_PKG_VERSION").to_string(), "1.2".to_string())
    }
}

/// What the user did with a notification, for the rest of the shell to act on.
pub struct Chosen {
    /// The application that sent it, to bring into view.
    pub app: String,
}

pub struct Center {
    pub open: bool,
    pub quiet: bool,
    /// Whether a notification that shows also sounds.
    pub sounds: bool,
    last_sound: Option<Instant>,
    notices: Vec<Notice>,
    inbox: Arc<Mutex<Inbox>>,
    /// The connection to D-Bus, when this shell is the notification service.
    bus: Option<zbus::blocking::Connection>,
    /// Ids for the desktop's own notices, kept apart from the ones D-Bus hands out.
    own_id: u32,
}

impl Center {
    /// Starts taking notifications. With `serve` off, or when another service already
    /// has the name, only the desktop's own notices are shown.
    pub fn new(ctx: egui::Context, quiet: bool, serve: bool) -> Center {
        let inbox: Arc<Mutex<Inbox>> = Arc::default();
        let bus = serve
            .then(|| {
                let service = Service { inbox: inbox.clone(), ctx };
                zbus::blocking::connection::Builder::session()?
                    .name(NAME)?
                    .serve_at(PATH, service)?
                    .build()
            })
            .and_then(|built| built.map_err(|e| eprintln!("des-ui: not taking applications' notifications: {e}")).ok());
        Center { open: false, quiet, sounds: false, last_sound: None, notices: Vec::new(), inbox, bus, own_id: u32::MAX / 2 }
    }

    /// A notice from the desktop itself.
    pub fn post(&mut self, title: &str, body: &str, urgent: bool) {
        self.own_id = self.own_id.wrapping_add(1).max(u32::MAX / 2);
        self.add(
            Notice {
                id: self.own_id,
                app: "DEs-UI".to_string(),
                title: title.to_string(),
                body: body.to_string(),
                actions: Vec::new(),
                urgent,
                icon: None,
                sound: None,
                clock: clock_now(),
                stay: if urgent { None } else { Some(STAY) },
                shown: None,
                fresh: true,
                foreign: false,
            },
            0,
        );
    }

    fn add(&mut self, mut notice: Notice, replaces: u32) {
        // Quiet, only what is urgent interrupts; the rest goes straight to the list.
        notice.fresh = !self.quiet || notice.urgent;
        let sound = self.sound_for(&notice);
        match self.notices.iter_mut().find(|old| replaces != 0 && old.id == replaces) {
            // The same thing said again, such as progress, is no new interruption.
            Some(old) => *old = notice,
            None => {
                self.notices.push(notice);
                if let Some(sound) = sound {
                    self.last_sound = Some(Instant::now());
                    play(&sound);
                }
            }
        }
        if self.notices.len() > KEPT {
            let gone = self.notices.remove(0);
            self.tell_closed(&gone, DISMISSED);
        }
    }

    /// The sound to make for a notification, if any: only for one that shows, not too
    /// soon after another, and never one it asked to be spared.
    fn sound_for(&self, notice: &Notice) -> Option<PathBuf> {
        if !self.sounds || !notice.fresh || self.last_sound.is_some_and(|at| at.elapsed() < SOUND_GAP) {
            return None;
        }
        match &notice.sound {
            Some(asked) => asked.clone(),
            None => Some(Path::new(SOUNDS).join(if notice.urgent { "dialog-warning.oga" } else { "message.oga" })),
        }
    }

    /// Takes in what arrived over D-Bus since the last frame.
    pub fn pump(&mut self) {
        let (arrived, withdrawn) = {
            let mut inbox = self.inbox.lock().unwrap();
            (std::mem::take(&mut inbox.arrived), std::mem::take(&mut inbox.withdrawn))
        };
        for (notice, replaces) in arrived {
            self.add(notice, replaces);
        }
        for id in withdrawn {
            if let Some(index) = self.notices.iter().position(|notice| notice.id == id) {
                let gone = self.notices.remove(index);
                self.tell_closed(&gone, WITHDRAWN);
            }
        }
    }

    /// How many have not been looked at: those that came while their toast could not
    /// be shown or was left to run out.
    pub fn count(&self) -> usize {
        self.notices.len()
    }

    fn signal<B: serde::Serialize + zbus::zvariant::DynamicType>(&self, name: &str, body: &B) {
        if let Some(bus) = &self.bus {
            let _ = bus.emit_signal(None::<zbus::names::BusName>, PATH, NAME, name, body);
        }
    }

    fn tell_closed(&self, notice: &Notice, reason: u32) {
        if notice.foreign {
            self.signal("NotificationClosed", &(notice.id, reason));
        }
    }

    /// Removes a notification for good, telling its sender.
    fn dismiss(&mut self, id: u32) {
        if let Some(index) = self.notices.iter().position(|notice| notice.id == id) {
            let gone = self.notices.remove(index);
            self.tell_closed(&gone, DISMISSED);
        }
    }

    /// Carries out one of a notification's actions, which also settles it.
    fn act(&mut self, id: u32, key: &str) -> Option<Chosen> {
        let notice = self.notices.iter().find(|notice| notice.id == id)?.clone();
        if notice.foreign {
            self.signal("ActionInvoked", &(id, key));
        }
        self.dismiss(id);
        Some(Chosen { app: notice.app })
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Draws the toasts, hanging from `corner` (their top right). Returns the part of
    /// the window they cover, for the compositor to lift above the applications, and
    /// what the user chose to act on.
    pub fn toasts(&mut self, ctx: &egui::Context, theme: &Theme, corner: egui::Pos2) -> (Option<Rect>, Option<Chosen>) {
        // Those whose time is up leave the screen and stay in the list.
        for notice in &mut self.notices {
            if let (Some(shown), Some(stay)) = (notice.shown, notice.stay) {
                if shown.elapsed() >= stay {
                    notice.fresh = false;
                    notice.shown = None;
                }
            }
        }
        let showing: Vec<Notice> = self.notices.iter().filter(|notice| notice.fresh).take(AT_ONCE).cloned().collect();
        if showing.is_empty() {
            return (None, None);
        }
        for notice in &mut self.notices {
            if notice.shown.is_none() && showing.iter().any(|shown| shown.id == notice.id) {
                notice.shown = Some(Instant::now());
            }
        }

        let mut covered: Option<Rect> = None;
        let mut chosen = None;
        let mut top = corner.y;
        for notice in &showing {
            let area = Area::new(Id::new(("toast", notice.id)))
                .order(Order::Foreground)
                .fixed_pos(pos2(corner.x - TOAST_WIDTH, top))
                .show(ctx, |ui| self.toast(ui, theme, notice));
            let rect = area.response.rect;
            top = rect.bottom() + 8.0;
            covered = Some(covered.map_or(rect, |all| all.union(rect)));
            match area.inner {
                Deed::Nothing => {}
                Deed::Hovering => {
                    // Under the pointer a toast does not run out.
                    if let Some(notice) = self.notices.iter_mut().find(|n| n.id == notice.id) {
                        notice.shown = Some(Instant::now());
                    }
                }
                Deed::Put { away: true } => self.dismiss(notice.id),
                Deed::Put { away: false } => {
                    if let Some(notice) = self.notices.iter_mut().find(|n| n.id == notice.id) {
                        notice.fresh = false;
                        notice.shown = None;
                    }
                }
                Deed::Act(key) => chosen = self.act(notice.id, &key),
            }
        }
        // The line that shows a toast's time running out has to keep moving.
        ctx.request_repaint_after(Duration::from_millis(50));
        (covered.map(|rect| rect.expand(1.0)), chosen)
    }

    fn toast(&self, ui: &mut Ui, theme: &Theme, notice: &Notice) -> Deed {
        let accent = if notice.urgent { ui::CAUTION } else { theme.main };
        let mut deed = Deed::Nothing;
        let frame = egui::Frame::new().fill(ui::PANEL).stroke(Stroke::new(1.0, ui::LINE_STRONG)).inner_margin(14.0);
        let shown = frame.show(ui, |ui| {
            ui.set_width(TOAST_WIDTH - 28.0);
            ui.spacing_mut().item_spacing.y = 5.0;
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(6.0, 6.0), Sense::hover());
                ui.painter().rect_filled(dot, 0.0, accent);
                ui::tracked(ui, &crate::panels::truncate(&notice.app.to_uppercase(), 24), ui::display_bold(10.0), accent, 2.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if close_mark(ui).clicked() {
                        deed = Deed::Put { away: true };
                    }
                    ui::text(ui, &notice.clock, ui::mono(10.0), ui::LABEL);
                });
            });
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if let Some(icon) = &notice.icon {
                    icon.show(ui, 40.0);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    if !notice.title.is_empty() {
                        wrapped(ui, &notice.title, ui::display_bold(14.0), ui::TEXT, 2);
                    }
                    if !notice.body.is_empty() {
                        wrapped(ui, &notice.body, ui::mono(12.0), ui::SECONDARY, 4);
                    }
                });
            });
            let offered: Vec<&(String, String)> = notice.actions.iter().filter(|(key, _)| key != "default").take(3).collect();
            if !offered.is_empty() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (key, label) in offered {
                        if ui::button(ui, theme, &crate::panels::truncate(&label.to_uppercase(), 16), None, false).clicked() {
                            deed = Deed::Act(key.clone());
                        }
                    }
                });
            }
        });
        let rect = shown.response.rect;
        ui::corners(ui.painter(), rect, 8.0, Stroke::new(1.5, accent));
        // A line along the bottom, running out with the toast's time.
        if let (Some(at), Some(stay)) = (self.notices.iter().find(|n| n.id == notice.id).and_then(|n| n.shown), notice.stay) {
            let left = 1.0 - (at.elapsed().as_secs_f32() / stay.as_secs_f32()).min(1.0);
            let line = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), pos2(rect.left() + rect.width() * left, rect.bottom()));
            ui.painter().rect_filled(line, 0.0, accent);
        }
        if deed == Deed::Nothing {
            let whole = ui.interact(rect, ui.id().with("whole"), Sense::click());
            if whole.clicked() {
                deed = match notice.default_action() {
                    Some(key) => Deed::Act(key.to_string()),
                    None => Deed::Put { away: false },
                };
            } else if ui.rect_contains_pointer(rect) {
                deed = Deed::Hovering;
            }
        }
        deed
    }

    /// The list of everything received, in place of the terminal. Returns what the
    /// user chose to act on, and whether quiet was switched.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme) -> (Option<Chosen>, Option<bool>) {
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.open = false;
            return (None, None);
        }
        // Opening the list is looking at them: nothing is left to pop up.
        for notice in &mut self.notices {
            notice.fresh = false;
            notice.shown = None;
        }
        let mut chosen = None;
        let mut switched = None;
        let margin = (rect.width() * 0.05).clamp(16.0, 56.0);
        let area = Rect::from_min_max(rect.min + vec2(margin, 28.0), rect.max - vec2(margin, 14.0));
        ui.allocate_new_ui(UiBuilder::new().max_rect(area), |ui| {
            ui.horizontal(|ui| {
                ui::tracked(ui, "NOTIFICATIONS", ui::display_bold(22.0), ui::TEXT, 3.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if ui::button(ui, theme, "DONE", Some("ESC"), false).clicked() {
                        self.open = false;
                    }
                    if !self.notices.is_empty() && ui::button(ui, theme, "CLEAR ALL", None, false).clicked() {
                        for notice in std::mem::take(&mut self.notices) {
                            self.tell_closed(&notice, DISMISSED);
                        }
                    }
                    let quiet = ui::button(ui, theme, "DO NOT DISTURB", None, self.quiet)
                        .on_hover_text("Keep notifications in this list without showing them as they arrive. Urgent ones still show.");
                    if quiet.clicked() {
                        self.quiet = !self.quiet;
                        switched = Some(self.quiet);
                    }
                });
            });
            ui.add_space(16.0);
            if self.bus.is_none() {
                ui::text(ui, "Another program is taking applications' notifications; only the desktop's own are shown here.", ui::mono(11.0), ui::CAUTION);
                ui.add_space(8.0);
            }
            if self.notices.is_empty() {
                ui::text(ui, "Nothing has come in.", ui::mono(13.0), ui::LABEL);
                return;
            }
            let mut put_away = None;
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for notice in self.notices.iter().rev() {
                    match row(ui, theme, notice) {
                        Deed::Put { .. } => put_away = Some(notice.id),
                        Deed::Act(key) => chosen = Some((notice.id, key)),
                        _ => {}
                    }
                }
            });
            if let Some(id) = put_away {
                self.dismiss(id);
            }
        });
        let chosen = chosen.and_then(|(id, key)| self.act(id, &key));
        if chosen.is_some() {
            self.open = false;
        }
        (chosen, switched)
    }
}

/// What the desktop itself gives notice of, as it was when last looked at.
#[derive(Default)]
pub struct Watch {
    battery: Option<u8>,
    online: Option<bool>,
    screens: Option<usize>,
    /// When the network was last seen to change, to say so only once it has settled.
    since: Option<Instant>,
    /// When the warning went out that the machine will sleep to save what is left.
    sleep_warned: Option<Instant>,
    /// Whether that sleep has been asked for since the battery last charged.
    slept: bool,
}

/// At or below this, on battery, the machine is put to sleep before it dies.
const NEARLY_EMPTY: u8 = 4;
/// How long the warning stands before that.
const SLEEP_WARNING: Duration = Duration::from_secs(60);

impl Watch {
    /// Gives notice of what changed. Returns true when the battery is so nearly
    /// empty that the machine should be put to sleep now.
    pub fn check(&mut self, center: &mut Center, battery: Option<Battery>, online: bool, screens: Option<usize>) -> bool {
        let mut sleep_now = false;
        if let Some(now) = battery {
            if now.plugged {
                // Charging again: the next time it runs low is a new occasion.
                if self.sleep_warned.take().is_some() && !self.slept {
                    center.post("Charging", "The machine will stay awake.", false);
                }
                self.slept = false;
            } else {
                if let Some(before) = self.battery {
                    for (level, urgent) in [(15, false), (8, true)] {
                        if before > level && now.percent <= level {
                            let left = now.minutes.map_or(String::new(), |minutes| format!(" About {} left.", crate::power::duration(minutes).to_lowercase()));
                            center.post("Battery low", &format!("{}%.{left} Plug in soon.", now.percent), urgent);
                        }
                    }
                }
                if now.percent <= NEARLY_EMPTY && !self.slept {
                    match self.sleep_warned {
                        None => {
                            self.sleep_warned = Some(Instant::now());
                            center.post("Battery almost empty", "Going to sleep in a minute, to keep your work. Plug in to stay awake.", true);
                        }
                        Some(at) if at.elapsed() >= SLEEP_WARNING => {
                            self.sleep_warned = None;
                            self.slept = true;
                            sleep_now = true;
                        }
                        Some(_) => {}
                    }
                }
            }
            self.battery = Some(now.percent);
        }

        // A connection that drops for a moment and returns is not worth a word.
        match self.online {
            None => self.online = Some(online),
            Some(before) if before != online => match self.since {
                None => self.since = Some(Instant::now()),
                Some(since) if since.elapsed() > Duration::from_secs(4) => {
                    self.online = Some(online);
                    self.since = None;
                    if online {
                        center.post("Back online", "", false);
                    } else {
                        center.post("Offline", "The network connection was lost.", false);
                    }
                }
                Some(_) => {}
            },
            Some(_) => self.since = None,
        }

        if let (Some(before), Some(now)) = (self.screens, screens) {
            if now > before {
                center.post("Display connected", &format!("{now} displays in use."), false);
            } else if now < before {
                center.post("Display disconnected", "", false);
            }
        }
        self.screens = screens.or(self.screens);
        sleep_now
    }
}

/// What was done to a notification this frame.
#[derive(Clone, PartialEq)]
enum Deed {
    Nothing,
    Hovering,
    /// Take it off the screen; `away` for good, otherwise it stays in the list.
    Put { away: bool },
    Act(String),
}

/// The small cross that puts a notification away.
fn close_mark(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    let ink = if response.hovered() { ui::TEXT } else { ui::LABEL };
    let arm = 3.5;
    let c = rect.center();
    ui.painter().line_segment([c - vec2(arm, arm), c + vec2(arm, arm)], Stroke::new(1.5, ink));
    ui.painter().line_segment([c + vec2(-arm, arm), c + vec2(arm, -arm)], Stroke::new(1.5, ink));
    response
}

/// Text that wraps to the width at hand, cut off after `rows` lines.
fn wrapped(ui: &mut Ui, text: &str, font: egui::FontId, color: egui::Color32, rows: usize) {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), font, color, ui.available_width());
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = false;
    let galley = ui.fonts(|fonts| fonts.layout_job(job));
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley, color);
}

/// One notification in the list.
fn row(ui: &mut Ui, theme: &Theme, notice: &Notice) -> Deed {
    let accent = if notice.urgent { ui::CAUTION } else { theme.main };
    let mut deed = Deed::Nothing;
    let top = ui.cursor().top();
    let width = ui.available_width();
    let inner = ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui::text(ui, &notice.clock, ui::mono(11.0), ui::LABEL);
            ui::tracked(ui, &crate::panels::truncate(&notice.app.to_uppercase(), 28), ui::display_bold(10.0), accent, 2.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                if close_mark(ui).clicked() {
                    deed = Deed::Put { away: true };
                }
            });
        });
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 12.0;
            let mut taken = 40.0;
            if let Some(icon) = &notice.icon {
                icon.show(ui, 36.0);
                taken += 48.0;
            }
            ui.vertical(|ui| {
                ui.set_width((width - taken).max(40.0));
                if !notice.title.is_empty() {
                    wrapped(ui, &notice.title, ui::display_bold(14.0), ui::TEXT, 2);
                }
                if !notice.body.is_empty() {
                    wrapped(ui, &notice.body, ui::mono(12.0), ui::SECONDARY, 6);
                }
            });
        });
        ui.add_space(10.0);
    });
    let rect = Rect::from_min_max(pos2(inner.response.rect.left(), top), pos2(inner.response.rect.left() + width, inner.response.rect.bottom()));
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, ui::TRACK));
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, ui::LINE_STRONG), StrokeKind::Inside);
    }
    if deed == Deed::Nothing {
        if let Some(key) = notice.default_action() {
            if ui.interact(rect, ui.id().with(("notice", notice.id)), Sense::click()).clicked() {
                deed = Deed::Act(key.to_string());
            }
        }
    }
    deed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_markup_and_entities() {
        assert_eq!(plain("<b>Ada</b>: see <a href=\"https://x\">this</a>"), "Ada: see this");
        assert_eq!(plain("one<br/>two"), "one\ntwo");
        assert_eq!(plain("a &lt; b &amp;&amp; c &gt; d"), "a < b && c > d");
        assert_eq!(plain("2 < 3"), "2 < 3");
        assert_eq!(plain("  plain  "), "plain");
    }

    #[test]
    fn reads_pictures_sent_as_pixels() {
        // Two by two, three bytes a pixel, each row padded to eight bytes.
        let data = [1, 2, 3, 4, 5, 6, 0, 0, 7, 8, 9, 10, 11, 12];
        let icon = Icon::from_raw(2, 2, 8, 3, &data).unwrap();
        assert_eq!((icon.width, icon.height), (2, 2));
        assert_eq!(icon.rgba, [1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255]);
        assert!(Icon::from_raw(2, 2, 8, 3, &data[..10]).is_none(), "too few pixels");
        assert!(Icon::from_raw(2, 2, 4, 3, &data).is_none(), "rows shorter than their pixels");
        assert!(Icon::from_raw(0, 2, 8, 3, &data).is_none());
        assert!(Icon::from_raw(2, 2, 8, 2, &data).is_none(), "not a kind of pixel known here");
        // A large one is kept small.
        let large = vec![9u8; 400 * 200 * 4];
        let icon = Icon::from_raw(400, 200, 1600, 4, &large).unwrap();
        assert_eq!((icon.width, icon.height), (96, 48));
    }

    #[test]
    fn finds_icons_by_name_and_sounds_by_hint() {
        let root = std::env::temp_dir().join(format!("des-icons-{}", std::process::id()));
        let folder = root.join("hicolor/48x48/apps");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("editor.png"), b"").unwrap();
        let roots = [root.clone()];
        assert_eq!(find_icon("editor", &roots), Some(folder.join("editor.png")));
        assert_eq!(find_icon("../editor", &roots), None);
        assert_eq!(find_icon("", &roots), None);
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(asked_sound(true, Some("/a.oga"), Some("bell")), Some(None), "asked for silence");
        assert_eq!(asked_sound(false, Some("/a.oga"), Some("bell")), Some(Some(PathBuf::from("/a.oga"))));
        assert_eq!(asked_sound(false, None, Some("bell")), Some(Some(PathBuf::from("/usr/share/sounds/freedesktop/stereo/bell.oga"))));
        assert_eq!(asked_sound(false, None, Some("../../etc/passwd")), None);
        assert_eq!(asked_sound(false, None, None), None);
    }

    #[test]
    fn sounds_only_for_what_shows_and_not_in_bursts() {
        let mut center = center();
        center.post("One", "", false);
        let notice = center.notices[0].clone();
        assert_eq!(center.sound_for(&notice), None, "sounds are off");
        center.sounds = true;
        assert_eq!(center.sound_for(&notice), Some(PathBuf::from("/usr/share/sounds/freedesktop/stereo/message.oga")));
        let mut urgent = notice.clone();
        urgent.urgent = true;
        assert!(center.sound_for(&urgent).unwrap().ends_with("dialog-warning.oga"));
        let mut spared = notice.clone();
        spared.sound = Some(None);
        assert_eq!(center.sound_for(&spared), None);
        let mut unseen = notice.clone();
        unseen.fresh = false;
        assert_eq!(center.sound_for(&unseen), None, "kept off the screen, kept quiet");
        center.last_sound = Some(Instant::now());
        assert_eq!(center.sound_for(&notice), None, "too soon after another");
    }

    #[test]
    fn pairs_actions() {
        let flat = ["default", "Open", "reply", "Reply", "dangling"].map(str::to_string);
        assert_eq!(pair_actions(&flat), vec![("default".into(), "Open".into()), ("reply".into(), "Reply".into())]);
    }

    fn center() -> Center {
        Center { open: false, quiet: false, sounds: false, last_sound: None, notices: Vec::new(), inbox: Arc::default(), bus: None, own_id: u32::MAX / 2 }
    }

    #[test]
    fn quiet_keeps_all_but_the_urgent_off_the_screen() {
        let mut center = center();
        center.quiet = true;
        center.post("Download finished", "", false);
        center.post("Battery low", "5% left", true);
        assert_eq!(center.count(), 2);
        let fresh: Vec<&str> = center.notices.iter().filter(|n| n.fresh).map(|n| n.title.as_str()).collect();
        assert_eq!(fresh, ["Battery low"]);
    }

    #[test]
    fn a_nearly_empty_battery_warns_then_asks_for_sleep_once() {
        let mut center = center();
        let mut watch = Watch::default();
        let battery = |percent, plugged| Some(Battery { percent, plugged, minutes: None });
        assert!(!watch.check(&mut center, battery(20, false), true, None));
        assert!(!watch.check(&mut center, battery(14, false), true, None));
        assert_eq!(center.notices.last().unwrap().title, "Battery low");
        assert!(!watch.check(&mut center, battery(4, false), true, None));
        assert_eq!(center.notices.last().unwrap().title, "Battery almost empty");
        // The warning has stood for its minute.
        watch.sleep_warned = Some(Instant::now() - SLEEP_WARNING);
        assert!(watch.check(&mut center, battery(4, false), true, None));
        assert!(!watch.check(&mut center, battery(3, false), true, None), "asked for once, not again on waking");
        // Plugged in and run down again: a new occasion.
        assert!(!watch.check(&mut center, battery(5, true), true, None));
        assert!(!watch.check(&mut center, battery(4, false), true, None));
        assert!(watch.sleep_warned.is_some());
    }

    #[test]
    fn a_replacement_takes_the_place_of_what_it_replaces() {
        let mut center = center();
        let notice = |id, title: &str| Notice {
            id, app: "Brave".into(), title: title.into(), body: String::new(), actions: Vec::new(), urgent: false,
            icon: None,
            sound: None,
            clock: "12:00".into(), stay: Some(STAY), shown: None, fresh: true, foreign: false,
        };
        center.add(notice(7, "1 new message"), 0);
        center.add(notice(8, "other"), 0);
        center.add(notice(7, "2 new messages"), 7);
        let titles: Vec<&str> = center.notices.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, ["2 new messages", "other"]);
        center.dismiss(7);
        assert_eq!(center.count(), 1);
    }
}
