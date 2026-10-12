//! The music player: what is playing in the right column, and a full view of the
//! user's Spotify library in the workspace.
//!
//! The playing is done by `khadi-music`, a service of its own; this is its remote
//! control. The shell starts the service when it is not running, and offers the
//! sign-in when the service says nobody is signed in.

use crate::theme::Theme;
use crate::ui;
use eframe::egui::{
    self, Align, Align2, Key, Layout, Modifiers, Rect, Response, ScrollArea, Sense, Shape, Stroke,
    StrokeKind, TextEdit, TextureHandle, Ui, UiBuilder, pos2, vec2,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub uri: String,
    pub name: String,
    pub artists: String,
    pub album: String,
    pub duration_ms: u32,
    pub cover: Option<String>,
}

/// What the service last said is playing, and when it said so.
#[derive(Debug, Clone, Default)]
pub struct Now {
    pub track: Option<Track>,
    pub playing: bool,
    position_ms: u32,
    pub volume: u8,
    pub shuffle: bool,
    /// `off`, `all` or `one`.
    pub repeat: String,
    /// The device it is playing on, when that is not this one.
    pub device: Option<String>,
    /// Whether the track is among the liked songs, once known.
    pub liked: Option<bool>,
    at: Option<Instant>,
}

impl Now {
    /// Where in the track playback is at this moment.
    pub fn position_ms(&self) -> u32 {
        let length = self.track.as_ref().map_or(0, |track| track.duration_ms);
        let moved = match (self.playing, self.at) {
            (true, Some(at)) => at.elapsed().as_millis() as u32,
            _ => 0,
        };
        self.position_ms.saturating_add(moved).min(length)
    }

    pub fn parse(value: &Value) -> Now {
        let track = &value["track"];
        Now {
            track: track["uri"].as_str().map(|uri| Track {
                uri: uri.to_string(),
                name: track["name"].as_str().unwrap_or_default().to_string(),
                artists: track["artists"].as_str().unwrap_or_default().to_string(),
                album: track["album"].as_str().unwrap_or_default().to_string(),
                duration_ms: track["duration_ms"].as_u64().unwrap_or(0) as u32,
                cover: track["cover"].as_str().map(str::to_string),
            }),
            playing: value["playing"].as_bool().unwrap_or(false),
            position_ms: value["position_ms"].as_u64().unwrap_or(0) as u32,
            volume: value["volume"].as_u64().unwrap_or(0).min(100) as u8,
            shuffle: value["shuffle"].as_bool().unwrap_or(false),
            repeat: value["repeat"].as_str().unwrap_or("off").to_string(),
            device: value["device"].as_str().map(str::to_string),
            liked: value["liked"].as_bool(),
            at: Some(Instant::now()),
        }
    }
}

/// What a row in a list is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Track,
    Playlist,
    Album,
    Artist,
    /// Something that can play: this machine, a phone, a speaker.
    Device,
}

/// A row in a list: a track, or a playlist, album or artist to open.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub uri: String,
    /// The id of a playlist, album or artist, for asking what is in it.
    pub id: String,
    pub name: String,
    /// A track's or album's artists, or a playlist's owner.
    pub by: String,
    /// The album a track is on, or the year an album came out.
    pub album: String,
    pub duration_ms: u32,
    /// For a track: the ids of its album and of its first artist, for going to them.
    pub album_id: String,
    pub artist_id: String,
    /// The address of its picture.
    pub cover: Option<String>,
    /// For a device: whether it is the one playing, and whether it is this machine.
    pub active: bool,
    pub here: bool,
}

pub fn parse_items(value: &Value) -> Vec<Item> {
    value["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let kind = match item["kind"].as_str() {
                        Some("playlist") => Kind::Playlist,
                        Some("album") => Kind::Album,
                        Some("artist") => Kind::Artist,
                        Some("device") => Kind::Device,
                        _ => Kind::Track,
                    };
                    let text = |key: &str| item[key].as_str().unwrap_or_default().to_string();
                    Some(Item {
                        kind,
                        uri: item["uri"].as_str()?.to_string(),
                        id: text("id"),
                        name: text("name"),
                        by: if kind == Kind::Playlist { text("owner") } else { text("artists") },
                        album: text("album"),
                        duration_ms: item["duration_ms"].as_u64().unwrap_or(0) as u32,
                        album_id: text("album_id"),
                        artist_id: text("artist_id"),
                        cover: item["cover"].as_str().map(str::to_string),
                        active: item["active"] == true,
                        here: item["here"] == true,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A track's words, as the service gave them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lyrics {
    /// The track they belong to.
    pub track: String,
    /// Whether each line carries the moment it is sung.
    pub synced: bool,
    /// Each line, with that moment in milliseconds.
    pub lines: Vec<(u32, String)>,
    /// Why there are none, when there are none.
    pub error: Option<String>,
}

impl Lyrics {
    pub fn parse(value: &Value) -> Lyrics {
        Lyrics {
            track: value["track"].as_str().unwrap_or_default().to_string(),
            synced: value["synced"] == true,
            lines: value["lines"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|line| (line["ms"].as_u64().unwrap_or(0) as u32, line["text"].as_str().unwrap_or_default().to_string()))
                .collect(),
            error: value["error"].as_str().map(str::to_string),
        }
    }

    /// The line being sung `position` milliseconds in: the last one that has begun.
    pub fn current(&self, position: u32) -> Option<usize> {
        if !self.synced {
            return None;
        }
        self.lines.iter().rposition(|(at, _)| *at <= position)
    }
}

/// Minutes and seconds, as on a track listing.
pub fn clock(ms: u32) -> String {
    format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60)
}

#[derive(Debug, Clone, PartialEq)]
enum Link {
    Starting,
    Connected,
    NotSignedIn,
    SigningIn,
    /// The service cannot be run at all, and why.
    Unavailable(String),
}

struct Shared {
    link: Link,
    now: Now,
    /// Lists received, by `kind:key`, each with the reason it is empty if it failed.
    lists: HashMap<String, (Vec<Item>, Option<String>)>,
    /// The lists, by the same names, that have more after what was received.
    more: std::collections::HashSet<String>,
    /// The words last received.
    lyrics: Option<Lyrics>,
    /// What last went wrong with something asked of the service, and when.
    problem: Option<(String, Instant)>,
    /// Set to ask the connection thread to run a sign-in: the service's arguments.
    sign_in: Option<Vec<String>>,
    /// Set when a sign-in went through, so that lists are asked for afresh.
    refresh: bool,
    /// Set to call off a sign-in that is waiting on the browser.
    cancel: bool,
}

fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("DES_MUSIC_SOCKET") {
        return PathBuf::from(path);
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("khadi-music.sock")
}

/// The service's program: the one installed beside this one.
fn service_program() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("khadi-music")))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("khadi-music"))
}

/// Keeps a connection to the service for as long as the shell runs: starts the
/// service when it is not there, reads what it says, and passes on what `commands`
/// brings.
fn connection(shared: Arc<Mutex<Shared>>, commands: Receiver<String>, ctx: egui::Context) {
    let set = |link: Link| {
        shared.lock().unwrap().link = link;
        ctx.request_repaint();
    };
    loop {
        let wants_sign_in = shared.lock().unwrap().sign_in.take();
        if let Some(arguments) = wants_sign_in {
            set(Link::SigningIn);
            // The sign-in waits on the browser for as long as it takes, so it can be
            // called off from here.
            let mut signed_in = false;
            if let Ok(mut child) = Command::new(service_program()).args(&arguments).stdin(Stdio::null()).spawn() {
                signed_in = loop {
                    match child.try_wait() {
                        Ok(None) if std::mem::take(&mut shared.lock().unwrap().cancel) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break false;
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                        Ok(Some(status)) => break status.success(),
                        Err(_) => break false,
                    }
                };
            }
            if signed_in {
                let mut shared = shared.lock().unwrap();
                shared.lists.clear();
                shared.more.clear();
                shared.refresh = true;
            }
            // Connecting the library fails without undoing the sign-in itself.
            set(if signed_in || arguments[0] != "login" { Link::Starting } else { Link::NotSignedIn });
            continue;
        }
        if shared.lock().unwrap().link == Link::NotSignedIn {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }

        let Ok(stream) = UnixStream::connect(socket_path()) else {
            // Not running: start it, in a group of its own so that it outlives the shell.
            let spawned = Command::new(service_program())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn();
            match spawned {
                Err(e) => {
                    set(Link::Unavailable(format!("The music service could not be started: {e}")));
                    std::thread::sleep(Duration::from_secs(10));
                }
                Ok(mut child) => {
                    // It says at once if nobody is signed in; otherwise it takes a
                    // while to reach Spotify and start listening. Wait for whichever
                    // comes, rather than starting a second one beside it.
                    let started = Instant::now();
                    while started.elapsed() < Duration::from_secs(30)
                        && matches!(child.try_wait(), Ok(None))
                        && UnixStream::connect(socket_path()).is_err()
                    {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                    match child.try_wait() {
                        Ok(Some(status)) if status.code() == Some(3) => set(Link::NotSignedIn),
                        Ok(Some(_)) => {
                            set(Link::Unavailable("The music service stopped as soon as it started. Is this machine online?".to_string()));
                            std::thread::sleep(Duration::from_secs(10));
                            set(Link::Starting);
                        }
                        _ => {
                            std::thread::spawn(move || {
                                let _ = child.wait();
                            });
                        }
                    }
                }
            }
            continue;
        };

        set(Link::Connected);
        let Ok(mut writer) = stream.try_clone() else {
            continue;
        };
        let reading = shared.clone();
        let repaint = ctx.clone();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let mut shared = reading.lock().unwrap();
                match value["type"].as_str() {
                    Some("state") => shared.now = Now::parse(&value),
                    Some("lyrics") => shared.lyrics = Some(Lyrics::parse(&value)),
                    Some("problem") => {
                        shared.problem = value["text"].as_str().map(|text| (text.to_string(), Instant::now()));
                    }
                    Some("list") => {
                        let key = format!("{}:{}", value["kind"].as_str().unwrap_or_default(), value["key"].as_str().unwrap_or_default());
                        let error = value["error"].as_str().map(str::to_string);
                        let offset = value["offset"].as_u64().unwrap_or(0) as usize;
                        if value["more"] == true {
                            shared.more.insert(key.clone());
                        } else {
                            shared.more.remove(&key);
                        }
                        match shared.lists.get_mut(&key) {
                            // A further part goes after what is there, if it is the part that follows.
                            Some((items, _)) if offset > 0 => {
                                if items.len() == offset && error.is_none() {
                                    items.extend(parse_items(&value));
                                }
                            }
                            _ if offset > 0 => {}
                            _ => {
                                shared.lists.insert(key, (parse_items(&value), error));
                            }
                        }
                    }
                    _ => {}
                }
                drop(shared);
                repaint.request_repaint();
            }
        });
        // Pass commands on until the service goes away.
        while !reader.is_finished() {
            match commands.recv_timeout(Duration::from_millis(250)) {
                Ok(command) => {
                    if writer.write_all(command.as_bytes()).is_err() {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if shared.lock().unwrap().sign_in.is_some() {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        let _ = writer.shutdown(std::net::Shutdown::Both);
        let _ = reader.join();
        set(Link::Starting);
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Which list the view is showing.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Liked,
    /// What plays next.
    Queue,
    Playlist { id: String, name: String, uri: String },
    /// The albums saved to the library.
    Albums,
    Album { id: String, name: String, uri: String },
    /// An artist's albums.
    Artist { id: String, name: String },
    /// What can play, to choose among.
    Devices,
    /// The words of the track playing.
    Lyrics,
    Search(String),
}

impl Source {
    fn key(&self) -> String {
        match self {
            Source::Liked => "liked:".to_string(),
            Source::Queue => "queue:".to_string(),
            Source::Playlist { id, .. } => format!("tracks:{id}"),
            Source::Albums => "albums:".to_string(),
            Source::Album { id, .. } => format!("album:{id}"),
            Source::Artist { id, .. } => format!("artist:{id}"),
            Source::Devices => "devices:".to_string(),
            Source::Lyrics => "lyrics:".to_string(),
            Source::Search(query) => format!("search:{query}"),
        }
    }

    /// How to ask for the part of this list from `offset` on, for the lists that
    /// come a part at a time.
    fn further(&self, offset: usize) -> Option<Value> {
        let mut request = match self {
            Source::Liked | Source::Playlist { .. } | Source::Albums => self.request(),
            _ => return None,
        };
        request["offset"] = json!(offset);
        Some(request)
    }

    fn request(&self) -> Value {
        match self {
            Source::Liked => json!({ "cmd": "liked" }),
            Source::Queue => json!({ "cmd": "queue" }),
            Source::Playlist { id, .. } => json!({ "cmd": "tracks", "playlist": id }),
            Source::Albums => json!({ "cmd": "albums" }),
            Source::Album { id, .. } => json!({ "cmd": "album", "album": id }),
            Source::Artist { id, .. } => json!({ "cmd": "artist", "artist": id }),
            Source::Devices => json!({ "cmd": "devices" }),
            // Asked for by track, as each begins.
            Source::Lyrics => json!({ "cmd": "state" }),
            Source::Search(query) => json!({ "cmd": "search", "q": query }),
        }
    }

    /// What to tell the service so that a track plays as part of this list.
    fn context(&self) -> Option<String> {
        match self {
            Source::Liked => Some("liked".to_string()),
            Source::Playlist { uri, .. } | Source::Album { uri, .. } => Some(uri.clone()),
            Source::Search(_) | Source::Queue | Source::Albums | Source::Artist { .. } | Source::Devices | Source::Lyrics => None,
        }
    }
}

/// The pictures shown beside tracks, albums and the rest: fetched and made small in
/// the background, a few at a time, and kept on disk so that each is fetched once.
struct Covers {
    wanted: Sender<String>,
    ready: Receiver<(String, Option<egui::ColorImage>)>,
    /// Those asked for: `None` while on their way, and for good if they cannot be had.
    held: HashMap<String, Option<TextureHandle>>,
}

impl Covers {
    fn new(ctx: &egui::Context) -> Covers {
        let (wanted, queue) = channel::<String>();
        let (done, ready) = channel();
        let queue = Arc::new(Mutex::new(queue));
        for _ in 0..3 {
            let (queue, done, ctx) = (queue.clone(), done.clone(), ctx.clone());
            std::thread::spawn(move || {
                loop {
                    let next = queue.lock().unwrap().recv();
                    let Ok(url) = next else { return };
                    let pixels = fetch_cover(&url);
                    if done.send((url, pixels)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Covers { wanted, ready, held: HashMap::new() }
    }

    fn get(&mut self, ctx: &egui::Context, url: Option<&str>) -> Option<TextureHandle> {
        while let Ok((url, pixels)) = self.ready.try_recv() {
            let texture = pixels.map(|pixels| ctx.load_texture(format!("cover-{url}"), pixels, egui::TextureOptions::LINEAR));
            self.held.insert(url, texture);
        }
        let url = url?;
        match self.held.get(url) {
            Some(texture) => texture.clone(),
            None => {
                self.held.insert(url.to_string(), None);
                let _ = self.wanted.send(url.to_string());
                None
            }
        }
    }
}

/// Fetches a cover, or finds it where it was kept, and reads it.
fn fetch_cover(url: &str) -> Option<egui::ColorImage> {
    // Kept on disk under a name made from the address.
    let name: String = url.chars().rev().take(48).filter(|c| c.is_ascii_alphanumeric()).collect();
    let dir = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("khadi/music/covers");
    let path = dir.join(name);
    let fetched = path.exists()
        || (std::fs::create_dir_all(&dir).is_ok()
            && url.starts_with("https://")
            && Command::new("curl")
                .args(["-fsS", "-m", "10", "--proto", "=https", "-o"])
                .arg(&path)
                .arg(url)
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success()));
    if !fetched {
        // Whatever part arrived is no picture.
        let _ = std::fs::remove_file(&path);
        return None;
    }
    // The file's name says nothing of its kind, so that is read from its contents.
    let image = image::ImageReader::open(&path).and_then(|reader| reader.with_guessed_format()).ok()?.decode().ok()?;
    let image = image.thumbnail(320, 320).to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw()))
}

/// A transport control, drawn rather than lettered.
#[derive(Clone, Copy, PartialEq)]
enum Control {
    Previous,
    Play,
    Pause,
    Next,
}

fn control(ui: &mut Ui, theme: &Theme, which: Control, size: f32, lit: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let ink = if lit { theme.main } else if response.hovered() { ui::TEXT } else { ui::SECONDARY };
    let painter = ui.painter();
    let c = rect.center();
    let unit = size / 6.0;
    let triangle = |tip_x: f32, back_x: f32| {
        Shape::convex_polygon(
            vec![pos2(c.x + back_x, c.y - unit * 1.4), pos2(c.x + tip_x, c.y), pos2(c.x + back_x, c.y + unit * 1.4)],
            ink,
            Stroke::NONE,
        )
    };
    let bar = |x: f32| Rect::from_center_size(pos2(c.x + x, c.y), vec2(unit * 0.5, unit * 2.8));
    match which {
        Control::Play => {
            painter.add(triangle(unit * 1.3, -unit * 0.9));
        }
        Control::Pause => {
            painter.rect_filled(bar(-unit * 0.7), 0.0, ink);
            painter.rect_filled(bar(unit * 0.7), 0.0, ink);
        }
        Control::Next => {
            painter.add(triangle(unit * 0.8, -unit * 1.2));
            painter.rect_filled(bar(unit * 1.2), 0.0, ink);
        }
        Control::Previous => {
            painter.add(triangle(-unit * 0.8, unit * 1.2));
            painter.rect_filled(bar(-unit * 1.2), 0.0, ink);
        }
    }
    response
}

pub struct Music {
    pub open: bool,
    shared: Arc<Mutex<Shared>>,
    commands: Sender<String>,
    source: Source,
    query: String,
    /// The track that was playing when the queue was last asked for.
    queue_as_of: Option<String>,
    /// The track last added to the queue, and when: to say so on its row for a moment.
    queued: Option<(String, Instant)>,
    /// The client ID being typed, while the library is not connected.
    client_id: String,
    /// Lists already asked for, so that each is asked for once.
    asked: Vec<String>,
    /// The further part of a list last asked for: the list, and from where.
    asked_further: Option<(String, usize)>,
    /// The lists looked at before this one, for going back.
    trail: Vec<Source>,
    covers: Covers,
    /// When the devices were last asked for, while they are being looked at.
    devices_asked: Option<Instant>,
    /// The track whose words were last asked for.
    lyrics_asked: Option<String>,
    /// The line last brought into view, so that it is done once per line.
    lyrics_at: Option<usize>,
}

impl Music {
    pub fn new(ctx: egui::Context) -> Music {
        let shared = Arc::new(Mutex::new(Shared {
            link: Link::Starting,
            now: Now::default(),
            lists: HashMap::new(),
            more: std::collections::HashSet::new(),
            lyrics: None,
            problem: None,
            sign_in: None,
            refresh: false,
            cancel: false,
        }));
        let (commands, receiver) = channel();
        let for_thread = shared.clone();
        let covers = Covers::new(&ctx);
        std::thread::spawn(move || connection(for_thread, receiver, ctx));
        Music {
            open: false,
            shared,
            commands,
            source: Source::Liked,
            query: String::new(),
            client_id: String::new(),
            queue_as_of: None,
            queued: None,
            asked: Vec::new(),
            asked_further: None,
            trail: Vec::new(),
            covers,
            devices_asked: None,
            lyrics_asked: None,
            lyrics_at: None,
        }
    }

    fn sign_in(&self, arguments: Vec<String>) {
        let mut shared = self.shared.lock().unwrap();
        shared.cancel = false;
        shared.sign_in = Some(arguments);
    }

    fn send(&self, request: Value) {
        let _ = self.commands.send(format!("{request}\n"));
    }

    pub fn now(&self) -> Now {
        self.shared.lock().unwrap().now.clone()
    }

    /// Starts on the words of what is playing.
    pub fn show_lyrics(&mut self) {
        self.source = Source::Lyrics;
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Shows another list. Going into something from a list leaves a way back to it.
    fn go(&mut self, to: Source, deeper: bool) {
        if to == self.source {
            return;
        }
        if deeper {
            self.trail.push(self.source.clone());
            if self.trail.len() > 12 {
                self.trail.remove(0);
            }
        } else {
            self.trail.clear();
        }
        self.source = to;
    }

    /// Asks for a list unless it was asked for already.
    fn want(&mut self, key: String, request: Value) {
        if !self.asked.contains(&key) {
            self.asked.push(key);
            self.send(request);
        }
    }

    /// Draws a square for the cover: the picture, or a quiet placeholder.
    fn draw_cover(&mut self, ui: &mut Ui, size: f32, url: Option<&str>) {
        let texture = self.covers.get(ui.ctx(), url);
        let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, ui::PANEL);
        match texture {
            Some(texture) => {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter().image(texture.id(), rect, uv, egui::Color32::WHITE);
            }
            None => {
                ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, ui::LINE), StrokeKind::Inside);
            }
        }
    }

    /// The right column's section: what is playing, and the three controls. Returns
    /// true when the title was clicked, to open the full view.
    pub fn panel(&mut self, ui: &mut Ui, theme: &Theme) -> bool {
        let now = self.now();
        let Some(track) = now.track.clone() else {
            return false;
        };
        let mut open = false;
        ui.add_space(22.0);
        ui::section(ui, "NOW PLAYING", |ui| {
            if let Some(device) = now.device.as_ref().filter(|_| now.playing) {
                ui::meta(ui, &format!("ON {}", crate::panels::truncate(&device.to_uppercase(), 14)), theme.main);
            } else if now.playing {
                ui::meta(ui, "SPOTIFY", theme.main);
            } else {
                ui::meta(ui, "PAUSED", ui::LABEL);
            }
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            self.draw_cover(ui, 56.0, track.cover.as_deref());
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let fit = (ui.available_width() / 8.5) as usize;
                let title = ui::text(ui, &crate::panels::truncate(&track.name, fit), ui::display_bold(14.0), ui::TEXT);
                open = ui.interact(title.rect, ui.id().with("now-playing-title"), Sense::click()).clicked();
                ui::text(ui, &crate::panels::truncate(&track.artists, fit + 3), ui::mono(11.0), ui::SECONDARY);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if control(ui, theme, Control::Previous, 22.0, false).clicked() {
                        self.send(json!({ "cmd": "prev" }));
                    }
                    let which = if now.playing { Control::Pause } else { Control::Play };
                    if control(ui, theme, which, 22.0, true).clicked() {
                        self.send(json!({ "cmd": "toggle" }));
                    }
                    if control(ui, theme, Control::Next, 22.0, false).clicked() {
                        self.send(json!({ "cmd": "next" }));
                    }
                });
            });
        });
        let frac = now.position_ms() as f32 / track.duration_ms.max(1) as f32;
        ui::bar(ui, theme, 3.0, frac.min(CAUTION_FREE), 0.0);
        open
    }

    /// What is playing, for the lock screen: the cover, the track, how far along it
    /// is, and the three controls. Nothing when nothing is. It sits at the bottom of
    /// `within`, in the middle.
    pub fn lock_card(&mut self, ui: &mut Ui, theme: &Theme, within: Rect) {
        let now = self.now();
        let Some(track) = now.track.clone() else {
            return;
        };
        let size = vec2(460.0_f32.min(within.width()), 116.0);
        let mut card = Rect::from_center_size(pos2(within.center().x, within.bottom() - 40.0 - size.y / 2.0), size);
        // On a short screen the middle is taken by the password line: the corner then.
        if card.top() < within.center().y + 236.0 {
            card = Rect::from_min_size(within.right_bottom() - size, size);
        }
        ui.painter().rect_filled(card, 0.0, ui::PANEL);
        ui.painter().rect_stroke(card, 0.0, Stroke::new(1.0, ui::LINE), StrokeKind::Inside);
        ui::corners(ui.painter(), card, 10.0, Stroke::new(1.5, if now.playing { theme.main } else { ui::LINE_STRONG }));
        let mut inner = ui.new_child(UiBuilder::new().max_rect(card.shrink(14.0)).layout(Layout::left_to_right(Align::Center)));
        let ui = &mut inner;
        ui.spacing_mut().item_spacing.x = 16.0;
        self.draw_cover(ui, 88.0, track.cover.as_deref());
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let fit = (ui.available_width() / 8.6) as usize;
            let state = match (&now.device, now.playing) {
                (Some(device), true) => format!("PLAYING ON {}", device.to_uppercase()),
                (None, true) => "NOW PLAYING".to_string(),
                (_, false) => "PAUSED".to_string(),
            };
            ui::tracked(ui, &crate::panels::truncate(&state, fit), ui::display_bold(10.0), if now.playing { theme.main } else { ui::LABEL }, 2.0);
            ui::text(ui, &crate::panels::truncate(&track.name, fit), ui::display_bold(16.0), ui::TEXT);
            ui::text(ui, &crate::panels::truncate(&track.artists, fit + 4), ui::mono(11.0), ui::SECONDARY);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if control(ui, theme, Control::Previous, 26.0, false).clicked() {
                    self.send(json!({ "cmd": "prev" }));
                }
                let which = if now.playing { Control::Pause } else { Control::Play };
                if control(ui, theme, which, 30.0, true).clicked() {
                    self.send(json!({ "cmd": "toggle" }));
                }
                if control(ui, theme, Control::Next, 26.0, false).clicked() {
                    self.send(json!({ "cmd": "next" }));
                }
                ui.add_space(10.0);
                let position = now.position_ms();
                ui::text(ui, &format!("{} / {}", clock(position), clock(track.duration_ms)), ui::mono(11.0), ui::LABEL);
            });
            let frac = now.position_ms() as f32 / track.duration_ms.max(1) as f32;
            ui::bar(ui, theme, 3.0, frac.min(CAUTION_FREE), 0.0);
        });
        // The position keeps moving while it plays.
        if now.playing {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
    }

    /// The full view, in place of the terminal.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme) {
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.open = false;
            return;
        }
        let (link, now) = {
            let shared = self.shared.lock().unwrap();
            (shared.link.clone(), shared.now.clone())
        };
        let margin = (rect.width() * 0.05).clamp(16.0, 56.0);
        let area = Rect::from_min_max(rect.min + vec2(margin, 28.0), rect.max - vec2(margin, 14.0));
        ui.allocate_new_ui(UiBuilder::new().max_rect(area), |ui| {
            ui.horizontal(|ui| {
                ui::tracked(ui, "MUSIC", ui::display_bold(22.0), ui::TEXT, 3.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::button(ui, theme, "DONE", Some("ESC"), false).clicked() {
                        self.open = false;
                    }
                });
            });
            ui.add_space(16.0);

            match link {
                Link::Connected => self.library(ui, theme, &now),
                Link::Starting => {
                    ui::text(ui, "Starting the music service…", ui::mono(13.0), ui::SECONDARY);
                }
                Link::SigningIn => {
                    ui::text(ui, "Spotify's page is open in your browser. Finish there and come back.", ui::mono(13.0), ui::TEXT);
                    ui.add_space(18.0);
                    if ui::button(ui, theme, "CANCEL", None, false).clicked() {
                        self.shared.lock().unwrap().cancel = true;
                    }
                }
                Link::Unavailable(why) => {
                    ui::text(ui, &why, ui::mono(13.0), ui::CAUTION);
                }
                Link::NotSignedIn => {
                    ui::text(ui, "Play your Spotify library here, without opening Spotify.", ui::display(18.0), ui::TEXT);
                    ui.add_space(8.0);
                    ui::text(ui, "Signing in happens once, on Spotify's own page in your browser. It needs Spotify Premium.", ui::mono(12.0), ui::SECONDARY);
                    ui.add_space(18.0);
                    if ui::button(ui, theme, "SIGN IN TO SPOTIFY", None, true).clicked() {
                        self.sign_in(vec!["login".to_string()]);
                    }
                }
            }
        });
    }

    /// Shown in place of the library until it is connected: what to do, and where
    /// to put the client ID.
    fn connect_library(&mut self, ui: &mut Ui, theme: &Theme) {
        ui::text(ui, "One more step: connect your library.", ui::display(18.0), ui::TEXT);
        ui.add_space(8.0);
        for line in [
            "You are signed in. Spotify only lets an app of your own read your playlists,",
            "liked songs and search, so it takes a free one, made once:",
            "",
            "1  Open developer.spotify.com/dashboard and choose Create app.",
            "2  Give it any name. As its Redirect URI enter exactly:  http://127.0.0.1:8898/login",
            "3  Tick Web API, save, and copy the app's Client ID here.",
        ] {
            ui::text(ui, line, ui::mono(12.0), if line.starts_with(char::is_numeric) { ui::TEXT } else { ui::SECONDARY });
        }
        ui.add_space(14.0);
        let (field, _) = ui.allocate_exact_size(vec2(340.0, 36.0), Sense::hover());
        ui.painter().rect_filled(field, 0.0, ui::PANEL);
        ui.painter().rect_stroke(field, 0.0, Stroke::new(1.0, ui::LINE_STRONG), StrokeKind::Inside);
        ui.put(
            field.shrink2(vec2(10.0, 7.0)),
            TextEdit::singleline(&mut self.client_id).frame(false).font(ui::mono(13.0)).text_color(ui::TEXT).hint_text("client ID"),
        );
        ui.add_space(10.0);
        let id = self.client_id.trim().to_string();
        let valid = id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit());
        if !id.is_empty() && !valid {
            ui::text(ui, "A client ID is 32 letters and digits.", ui::mono(11.0), ui::CAUTION);
        }
        if ui::button(ui, theme, "CONNECT LIBRARY", None, valid).clicked() && valid {
            self.sign_in(vec!["library".to_string(), id]);
        }
    }

    fn library(&mut self, ui: &mut Ui, theme: &Theme, now: &Now) {
        if std::mem::take(&mut self.shared.lock().unwrap().refresh) {
            self.asked.clear();
            self.asked_further = None;
        }
        self.want("playlists:".to_string(), json!({ "cmd": "playlists" }));
        let source = self.source.clone();
        self.want(source.key(), source.request());
        // The queue moves on with every track, so it is asked for again each time.
        let playing_uri = now.track.as_ref().map(|track| track.uri.clone());
        if source == Source::Queue && self.queue_as_of != playing_uri {
            self.queue_as_of = playing_uri;
            self.send(json!({ "cmd": "queue" }));
        }
        // Devices come and go, so while they are looked at they are asked for again.
        if source == Source::Devices && self.devices_asked.is_none_or(|at| at.elapsed() > Duration::from_secs(6)) {
            self.devices_asked = Some(Instant::now());
            self.send(json!({ "cmd": "devices" }));
            ui.ctx().request_repaint_after(Duration::from_secs(6));
        }
        let (playlists, list, more, problem) = {
            let mut shared = self.shared.lock().unwrap();
            if shared.problem.as_ref().is_some_and(|(_, at)| at.elapsed() > Duration::from_secs(12)) {
                shared.problem = None;
            }
            (
                shared.lists.get("playlists:").cloned(),
                shared.lists.get(&source.key()).cloned(),
                shared.more.contains(&source.key()),
                shared.problem.as_ref().map(|(text, _)| text.clone()),
            )
        };

        let transport_height = 84.0;
        let problem_height = if problem.is_some() { 40.0 } else { 0.0 };
        let body_height = (ui.available_height() - transport_height - 12.0 - problem_height).max(80.0);
        let unconnected = matches!(&playlists, Some((_, Some(why))) if why.starts_with("Your library is not connected"));
        if unconnected {
            ui.allocate_ui_with_layout(vec2(ui.available_width(), body_height), Layout::top_down(Align::Min), |ui| {
                self.connect_library(ui, theme);
            });
            ui.add_space(12.0);
            self.transport(ui, theme, now, transport_height);
            return;
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 28.0;
            // Left: search, then the library.
            ui.allocate_ui_with_layout(vec2(240.0, body_height), Layout::top_down(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let (field, _) = ui.allocate_exact_size(vec2(240.0, 36.0), Sense::hover());
                ui.painter().rect_filled(field, 0.0, ui::PANEL);
                ui.painter().rect_stroke(field, 0.0, Stroke::new(1.0, ui::LINE_STRONG), StrokeKind::Inside);
                let edit = ui.put(
                    field.shrink2(vec2(10.0, 7.0)),
                    TextEdit::singleline(&mut self.query)
                        .frame(false)
                        .font(ui::mono(13.0))
                        .text_color(ui::TEXT)
                        .hint_text("search Spotify"),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !self.query.trim().is_empty() {
                    self.go(Source::Search(self.query.trim().to_string()), false);
                }
                ui.add_space(10.0);
                if side_row(ui, theme, "LIKED SONGS", self.source == Source::Liked) {
                    self.go(Source::Liked, false);
                }
                if side_row(ui, theme, "ALBUMS", self.source == Source::Albums) {
                    self.go(Source::Albums, false);
                }
                if side_row(ui, theme, "UP NEXT", self.source == Source::Queue) {
                    self.go(Source::Queue, false);
                    self.queue_as_of = None;
                }
                if side_row(ui, theme, "LYRICS", self.source == Source::Lyrics) {
                    self.go(Source::Lyrics, false);
                    self.lyrics_at = None;
                }
                if side_row(ui, theme, "DEVICES", self.source == Source::Devices) {
                    self.go(Source::Devices, false);
                    self.devices_asked = None;
                }
                ui.add_space(8.0);
                ui::tracked(ui, "PLAYLISTS", ui::display_bold(11.0), ui::LABEL, 3.0);
                match playlists {
                    None => {
                        ui::text(ui, "Loading…", ui::mono(12.0), ui::LABEL);
                    }
                    Some((_, Some(why))) => {
                        ui::text(ui, &why, ui::mono(11.0), ui::CAUTION);
                    }
                    Some((items, None)) => {
                        ScrollArea::vertical().id_salt("playlists").auto_shrink([false, false]).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            for playlist in &items {
                                let chosen = matches!(&self.source, Source::Playlist { id, .. } if *id == playlist.id);
                                if side_row(ui, theme, &playlist.name, chosen) {
                                    let to = Source::Playlist {
                                        id: playlist.id.clone(),
                                        name: playlist.name.clone(),
                                        uri: playlist.uri.clone(),
                                    };
                                    self.go(to, false);
                                }
                            }
                        });
                    }
                }
            });

            // Right: the list chosen.
            ui.allocate_ui_with_layout(vec2(ui.available_width(), body_height), Layout::top_down(Align::Min), |ui| {
                let title = match &source {
                    Source::Liked => "LIKED SONGS".to_string(),
                    Source::Queue => "UP NEXT".to_string(),
                    Source::Playlist { name, .. } | Source::Album { name, .. } => name.to_uppercase(),
                    Source::Albums => "ALBUMS".to_string(),
                    Source::Artist { name, .. } => format!("ALBUMS BY {}", name.to_uppercase()),
                    Source::Devices => "PLAY ON".to_string(),
                    Source::Lyrics => "LYRICS".to_string(),
                    Source::Search(query) => format!("RESULTS FOR “{}”", query.to_uppercase()),
                };
                let mut back = false;
                ui::section(ui, &crate::panels::truncate(&title, 48), |ui| {
                    if !self.trail.is_empty() {
                        back = ui::button(ui, theme, "BACK", None, false).clicked();
                    }
                    if let Some((items, None)) = &list {
                        let count = if more { format!("{} SO FAR", items.len()) } else { format!("{} SHOWN", items.len()) };
                        ui::meta(ui, &count, ui::LABEL);
                    }
                });
                if back {
                    if let Some(to) = self.trail.pop() {
                        self.source = to;
                    }
                    return;
                }
                if source == Source::Lyrics {
                    self.lyrics(ui, theme, now);
                    return;
                }
                match list {
                    None => {
                        ui::text(ui, "Loading…", ui::mono(12.0), ui::LABEL);
                    }
                    Some((_, Some(why))) if why.starts_with("The queue needs") => {
                        ui::text(ui, "The queue needs one more approval from Spotify, on its page in your browser.", ui::mono(12.0), ui::TEXT);
                        ui.add_space(12.0);
                        if ui::button(ui, theme, "APPROVE", None, true).clicked() {
                            self.sign_in(vec!["library".to_string()]);
                        }
                    }
                    Some((_, Some(why))) => {
                        ui::text(ui, &why, ui::mono(12.0), ui::CAUTION);
                    }
                    Some((items, None)) if items.is_empty() => {
                        let empty = match source {
                            Source::Queue => "Nothing is waiting to play.",
                            Source::Devices => "No device is online. Open Spotify on one and it shows up here.",
                            _ => "Nothing here.",
                        };
                        ui::text(ui, empty, ui::mono(12.0), ui::LABEL);
                    }
                    Some((items, None)) => {
                        let playing = now.track.as_ref().map(|track| track.uri.as_str());
                        ScrollArea::vertical().id_salt("tracks").auto_shrink([false, false]).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let in_queue = source == Source::Queue;
                            for (index, item) in items.iter().enumerate() {
                                let just_queued = self.queued.as_ref().is_some_and(|(uri, at)| *uri == item.uri && at.elapsed() < Duration::from_secs(2));
                                // In the queue a track may come up twice, and none of them is the one playing.
                                let lit = (!in_queue && playing == Some(item.uri.as_str())) || item.active;
                                // Only the rows in view have their pictures fetched.
                                let place = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ROW));
                                let cover = if ui.is_rect_visible(place) { self.covers.get(ui.ctx(), item.cover.as_deref()) } else { None };
                                let picked = ui.push_id(index, |ui| track_row(ui, theme, item, cover.as_ref(), lit, !in_queue, just_queued)).inner;
                                if picked == Picked::Queue {
                                    self.queued = Some((item.uri.clone(), Instant::now()));
                                    self.send(json!({ "cmd": "enqueue", "track": item.uri }));
                                    ui.ctx().request_repaint_after(Duration::from_secs(2));
                                }
                                // From a track to the album it is on, or to who made it.
                                if picked == Picked::Album && !item.album_id.is_empty() {
                                    let uri = format!("spotify:album:{}", item.album_id);
                                    self.go(Source::Album { id: item.album_id.clone(), name: item.album.clone(), uri }, true);
                                }
                                if picked == Picked::Artist && !item.artist_id.is_empty() {
                                    // Only the first of several artists can be gone to.
                                    let name = item.by.split(", ").next().unwrap_or_default().to_string();
                                    self.go(Source::Artist { id: item.artist_id.clone(), name }, true);
                                }
                                if picked != Picked::Row {
                                    continue;
                                }
                                let named = (item.id.clone(), item.name.clone(), item.uri.clone());
                                match item.kind {
                                    // Spotify offers no way to jump into the queue but one step at a time.
                                    Kind::Track if in_queue => {
                                        for _ in 0..=index {
                                            self.send(json!({ "cmd": "next" }));
                                        }
                                    }
                                    Kind::Track => self.send(json!({ "cmd": "play", "context": source.context(), "track": item.uri })),
                                    Kind::Playlist => self.go(Source::Playlist { id: named.0, name: named.1, uri: named.2 }, true),
                                    Kind::Album => self.go(Source::Album { id: named.0, name: named.1, uri: named.2 }, true),
                                    Kind::Artist => self.go(Source::Artist { id: named.0, name: named.1 }, true),
                                    Kind::Device => {
                                        if !item.active {
                                            self.send(json!({ "cmd": "transfer", "device": item.id }));
                                        }
                                    }
                                }
                            }
                            // The rest of a long list is fetched as its end comes into view.
                            if more {
                                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                                ui.painter().text(rect.left_center() + vec2(26.0, 0.0), Align2::LEFT_CENTER, "Loading more…", ui::mono(12.0), ui::LABEL);
                                let wanted = (source.key(), items.len());
                                if ui.is_rect_visible(rect) && self.asked_further.as_ref() != Some(&wanted) {
                                    if let Some(request) = source.further(items.len()) {
                                        self.send(request);
                                        self.asked_further = Some(wanted);
                                    }
                                }
                            }
                        });
                    }
                }
            });
        });

        if let Some(problem) = problem {
            ui.allocate_ui_with_layout(vec2(ui.available_width(), problem_height), Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui::text(ui, &crate::panels::truncate(&problem, 110), ui::mono(12.0), ui::CAUTION);
                // One more permission is asked for on Spotify's page, as for the library.
                if problem.contains("needs one more approval") && ui::button(ui, theme, "APPROVE", None, true).clicked() {
                    self.shared.lock().unwrap().problem = None;
                    self.sign_in(vec!["library".to_string()]);
                }
            });
        }
        ui.add_space(12.0);
        self.transport(ui, theme, now, transport_height);
    }

    /// The words of the track playing: the line being sung in the accent and kept in
    /// the middle, the rest around it. A click on a line goes to it.
    fn lyrics(&mut self, ui: &mut Ui, theme: &Theme, now: &Now) {
        let Some(track) = &now.track else {
            ui::text(ui, "Nothing is playing.", ui::mono(12.0), ui::LABEL);
            return;
        };
        if self.lyrics_asked.as_deref() != Some(track.uri.as_str()) {
            self.lyrics_asked = Some(track.uri.clone());
            self.lyrics_at = None;
            self.send(json!({ "cmd": "lyrics", "track": track.uri }));
        }
        let lyrics = self.shared.lock().unwrap().lyrics.clone().filter(|lyrics| lyrics.track == track.uri);
        let Some(lyrics) = lyrics else {
            ui::text(ui, "Looking for the words…", ui::mono(12.0), ui::LABEL);
            return;
        };
        if let Some(why) = &lyrics.error {
            ui::text(ui, why, ui::mono(12.0), ui::LABEL);
            return;
        }
        let current = lyrics.current(now.position_ms());
        if !lyrics.synced {
            ui::text(ui, "These words are not timed to the music.", ui::mono(11.0), ui::LABEL);
            ui.add_space(8.0);
        }
        let moved = current != self.lyrics_at;
        self.lyrics_at = current;
        let mut seek = None;
        ScrollArea::vertical().id_salt("lyrics").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.add_space(ui.available_height() * 0.3);
            for (index, (at, text)) in lyrics.lines.iter().enumerate() {
                let sung = current == Some(index);
                let (font, ink) = match (lyrics.synced, sung, current.is_some_and(|current| index < current)) {
                    (false, _, _) => (ui::display(17.0), ui::TEXT),
                    (true, true, _) => (ui::display_bold(22.0), theme.main),
                    (true, false, true) => (ui::display(17.0), ui::LABEL),
                    (true, false, false) => (ui::display(17.0), ui::SECONDARY),
                };
                // A line with no words is a stretch of music.
                let shown = if text.is_empty() { "· · ·" } else { text.as_str() };
                let galley = ui.painter().layout(shown.to_string(), font, ink, ui.available_width() - 12.0);
                let (rect, response) = ui.allocate_exact_size(galley.size(), if lyrics.synced { Sense::click() } else { Sense::hover() });
                let ink = if response.hovered() && !sung { ui::TEXT } else { ink };
                ui.painter().galley(rect.min, galley, ink);
                if response.clicked() {
                    seek = Some(*at);
                }
                // Follow the singing, but only as it moves on: scrolling by hand is let be until then.
                if sung && moved {
                    ui.scroll_to_rect(rect, Some(Align::Center));
                }
            }
            ui.add_space(ui.available_height().max(120.0) * 0.5);
        });
        if let Some(ms) = seek {
            self.send(json!({ "cmd": "seek", "ms": ms }));
        }
        // The line changes with the music, not with the pointer.
        if now.playing && lyrics.synced {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
    }

    /// The strip along the bottom of the view: the track, the controls, the position.
    fn transport(&mut self, ui: &mut Ui, theme: &Theme, now: &Now, height: f32) {
        let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        ui.painter().rect_filled(strip, 0.0, ui::PANEL);
        ui.painter().rect_stroke(strip, 0.0, Stroke::new(1.0, ui::LINE), StrokeKind::Inside);
        let mut inner = ui.new_child(UiBuilder::new().max_rect(strip.shrink(14.0)).layout(Layout::left_to_right(Align::Center)));
        let ui = &mut inner;
        ui.spacing_mut().item_spacing.x = 14.0;
        let Some(track) = &now.track else {
            ui::text(ui, "Nothing is playing. Pick a track above.", ui::mono(12.0), ui::LABEL);
            return;
        };
        self.draw_cover(ui, 56.0, track.cover.as_deref());
        let names = (strip.width() * 0.22).clamp(120.0, 260.0);
        ui.allocate_ui_with_layout(vec2(names, 56.0), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if now.device.is_none() {
                ui.add_space(6.0);
            }
            let fit = (names / 8.5) as usize;
            ui::text(ui, &crate::panels::truncate(&track.name, fit), ui::display_bold(15.0), ui::TEXT);
            ui::text(ui, &crate::panels::truncate(&track.artists, fit + 4), ui::mono(11.0), ui::SECONDARY);
            // Playing somewhere else: say where, and offer the way to change that.
            if let Some(device) = &now.device {
                let label = format!("ON {}", crate::panels::truncate(&device.to_uppercase(), fit.saturating_sub(4)));
                let said = ui::tracked(ui, &label, ui::display_bold(10.0), theme.main, 1.5);
                if ui.interact(said.rect, ui.id().with("playing-on"), Sense::click()).on_hover_text("Choose where to play").clicked() {
                    self.go(Source::Devices, false);
                    self.devices_asked = None;
                }
            }
        });
        ui.spacing_mut().item_spacing.x = 4.0;
        if control(ui, theme, Control::Previous, 32.0, false).clicked() {
            self.send(json!({ "cmd": "prev" }));
        }
        let which = if now.playing { Control::Pause } else { Control::Play };
        if control(ui, theme, which, 40.0, true).clicked() {
            self.send(json!({ "cmd": "toggle" }));
        }
        if control(ui, theme, Control::Next, 32.0, false).clicked() {
            self.send(json!({ "cmd": "next" }));
        }
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.x = 10.0;
        let position = now.position_ms();
        ui::text(ui, &clock(position), ui::mono(12.0), ui::TEXT);
        // What is left over, after the readings and controls to the right, is the bar.
        // In a narrow view the volume gives way: the keyboard's keys still set it.
        let roomy = strip.width() > 940.0;
        let bar_width = (ui.available_width() - if roomy { 610.0 } else { 380.0 }).max(60.0);
        let frac = position as f32 / track.duration_ms.max(1) as f32;
        if let Some(to) = ui::slider(ui, theme, bar_width, frac) {
            // Only on release: asking for every step of a drag would stutter.
            if ui.input(|i| i.pointer.any_released()) || !ui.input(|i| i.pointer.any_down()) {
                self.send(json!({ "cmd": "seek", "ms": (to * track.duration_ms as f32) as u32 }));
            }
        }
        ui::text(ui, &clock(track.duration_ms), ui::mono(12.0), ui::SECONDARY);
        ui.add_space(8.0);
        if ui::chip(ui, theme, "SHUFFLE", None, now.shuffle).clicked() {
            self.send(json!({ "cmd": "shuffle", "on": !now.shuffle }));
        }
        // Off, then the whole list over again, then the one track.
        let (label, next) = match now.repeat.as_str() {
            "all" => ("REPEAT", "one"),
            "one" => ("REPEAT 1", "off"),
            _ => ("REPEAT", "all"),
        };
        if ui::chip(ui, theme, label, None, now.repeat != "off").clicked() {
            self.send(json!({ "cmd": "repeat", "mode": next }));
        }
        let liked = now.liked == Some(true);
        if ui::chip(ui, theme, if liked { "LIKED" } else { "LIKE" }, None, liked).clicked() {
            self.send(json!({ "cmd": "like", "on": !liked }));
        }
        if !roomy {
            return;
        }
        let on_lyrics = self.source == Source::Lyrics;
        if ui::chip(ui, theme, "LYRICS", None, on_lyrics).clicked() {
            if on_lyrics {
                // Back to where the words were opened from.
                let back = self.trail.pop().unwrap_or(Source::Liked);
                self.source = back;
            } else {
                self.go(Source::Lyrics, true);
                self.lyrics_at = None;
            }
        }
        ui::text(ui, "VOL", ui::mono(11.0), ui::LABEL);
        if let Some(to) = ui::slider(ui, theme, 90.0, now.volume as f32 / 100.0) {
            let percent = (to * 100.0).round() as u8;
            if percent != now.volume {
                self.send(json!({ "cmd": "volume", "percent": percent }));
            }
        }
    }
}

/// The progress bar never shows the caution colour: a track near its end is no warning.
const CAUTION_FREE: f32 = ui::CAUTION_ABOVE;

/// A row in the left list. Returns true when clicked.
fn side_row(ui: &mut Ui, theme: &Theme, name: &str, chosen: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let painter = ui.painter_at(rect);
    if chosen {
        painter.rect_filled(rect, 0.0, theme.main);
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, ui::PANEL);
    }
    let (font, ink) = if chosen { (ui::display_bold(13.0), ui::GROUND) } else { (ui::display(13.0), ui::TEXT) };
    painter.text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, crate::panels::truncate(name, 26), font, ink);
    response.clicked()
}

/// What was clicked on a row: the row itself, or its way into the queue.
#[derive(Clone, Copy, PartialEq)]
enum Picked {
    Nothing,
    Row,
    Queue,
    /// The album a track is on.
    Album,
    /// Who a track is by.
    Artist,
}

/// What a row says in its second column: who a track is by, or what else the row is.
fn described(item: &Item) -> String {
    match item.kind {
        Kind::Track => item.by.clone(),
        Kind::Playlist if item.by.is_empty() => "playlist".to_string(),
        Kind::Playlist => format!("playlist by {}", item.by),
        Kind::Album if item.by.is_empty() => "album".to_string(),
        Kind::Album => format!("album by {}", item.by),
        Kind::Artist => "artist".to_string(),
        Kind::Device if item.here => format!("this machine · {}", item.by.to_lowercase()),
        Kind::Device => item.by.to_lowercase(),
    }
}

/// How tall a row of a list is.
const ROW: f32 = 44.0;

/// A track, or a playlist, album or artist to open. `queueable` rows carry a way to add
/// them to what plays next, and say `queued` for a moment after.
fn track_row(ui: &mut Ui, theme: &Theme, item: &Item, cover: Option<&TextureHandle>, playing: bool, queueable: bool, queued: bool) -> Picked {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    // Under the pointer, the row offers to queue the track where its length is.
    let mut add = None;
    let track = item.kind == Kind::Track;
    if queueable && track && (queued || ui.rect_contains_pointer(rect)) {
        let spot = Rect::from_min_max(pos2(rect.right() - 92.0, rect.top()), rect.right_bottom());
        add = Some((spot, ui.interact(spot, ui.id().with("queue"), Sense::click())));
    }
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, 0.0, ui::PANEL);
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, ui::TRACK));
    let ink = if playing { theme.main } else { ui::TEXT };
    let mark = Rect::from_center_size(rect.left_center() + vec2(10.0, 0.0), vec2(6.0, 6.0));
    if playing {
        painter.rect_filled(mark, 0.0, theme.main);
    }
    // Its picture, where it has one; a device has none, and none is drawn for it.
    let lead = if item.kind == Kind::Device { 26.0 } else { 64.0 };
    if item.kind != Kind::Device {
        let square = Rect::from_center_size(rect.left_center() + vec2(40.0, 0.0), vec2(32.0, 32.0));
        match cover {
            Some(texture) => {
                painter.image(texture.id(), square, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            None => {
                painter.rect_filled(square, 0.0, ui::PANEL);
                painter.rect_stroke(square, 0.0, Stroke::new(1.0, ui::LINE), StrokeKind::Inside);
            }
        }
    }
    let width = rect.width();
    // How many characters fit in a column this many points wide.
    let chars = |points: f32| ((points - 14.0).max(0.0) / 8.0) as usize;
    let with_album = width > 620.0;
    let by_width = if with_album { width * 0.26 } else { width * 0.54 - 64.0 };
    painter.text(rect.left_center() + vec2(lead, 0.0), Align2::LEFT_CENTER, crate::panels::truncate(&item.name, chars(width * 0.46 - lead)), ui::mono(13.0), ink);
    let by = described(item);
    // A track's artist and album are ways to go to them.
    let mut went = None;
    let mut link = |ui: &mut Ui, at: f32, text: String, quiet: egui::Color32, to: Picked, id: &str| {
        let from = rect.left_center() + vec2(at, 0.0);
        let galley = painter.layout_no_wrap(text, ui::mono(12.0), quiet);
        let spot = Rect::from_min_size(from - vec2(0.0, galley.size().y / 2.0), galley.size()).expand2(vec2(2.0, 6.0));
        let over = ui.interact(spot, ui.id().with(id), Sense::click());
        if over.hovered() {
            painter.hline(spot.shrink2(vec2(2.0, 0.0)).x_range(), spot.bottom() - 5.0, Stroke::new(1.0, ui::TEXT));
        }
        painter.galley(from - vec2(0.0, galley.size().y / 2.0), galley, if over.hovered() { ui::TEXT } else { quiet });
        if over.clicked() {
            went = Some(to);
        }
    };
    let by_text = crate::panels::truncate(&by, chars(by_width));
    if track && !item.artist_id.is_empty() {
        link(ui, width * 0.46, by_text, ui::SECONDARY, Picked::Artist, "artist");
    } else {
        painter.text(rect.left_center() + vec2(width * 0.46, 0.0), Align2::LEFT_CENTER, by_text, ui::mono(12.0), ui::SECONDARY);
    }
    if with_album {
        let album_text = crate::panels::truncate(&item.album, chars(width * 0.28 - 64.0));
        if track && !item.album_id.is_empty() {
            link(ui, width * 0.72, album_text, ui::LABEL, Picked::Album, "album");
        } else {
            painter.text(rect.left_center() + vec2(width * 0.72, 0.0), Align2::LEFT_CENTER, album_text, ui::mono(12.0), ui::LABEL);
        }
    }
    if let Some(went) = went {
        return went;
    }
    if let Some((spot, adding)) = add {
        let (label, ink) = match (queued, adding.hovered()) {
            (true, _) => ("QUEUED", theme.main),
            (false, true) => ("+ QUEUE", ui::TEXT),
            (false, false) => ("+ QUEUE", ui::SECONDARY),
        };
        painter.text(spot.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, label, ui::display_bold(11.0), ink);
        if adding.clicked() {
            return Picked::Queue;
        }
        return if response.clicked() { Picked::Row } else { Picked::Nothing };
    }
    let end = match item.kind {
        Kind::Track => clock(item.duration_ms),
        Kind::Device if item.active => "PLAYING".to_string(),
        Kind::Device => "SWITCH TO".to_string(),
        _ => "OPEN".to_string(),
    };
    painter.text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, end, ui::mono(12.0), ui::SECONDARY);
    if response.clicked() { Picked::Row } else { Picked::Nothing }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_service_says() {
        let now = Now::parse(&json!({"type": "state", "playing": false, "position_ms": 61000, "volume": 80, "shuffle": true,
            "track": {"uri": "spotify:track:1", "name": "So What", "artists": "Miles Davis", "album": "Kind of Blue", "duration_ms": 562000, "cover": null}}));
        assert_eq!(now.track.as_ref().unwrap().name, "So What");
        assert_eq!(now.position_ms(), 61000, "paused: the position stands still");
        assert!(now.shuffle);
        assert_eq!(Now::parse(&json!({"type": "state", "track": null})).track, None);

        let items = parse_items(&json!({"items": [
            {"kind": "track", "uri": "spotify:track:1", "name": "So What", "artists": "Miles Davis", "album": "Kind of Blue", "duration_ms": 562000},
            {"kind": "playlist", "uri": "spotify:playlist:p", "id": "p", "name": "Focus", "owner": "ada"},
            {"kind": "track", "name": "no address"}]}));
        assert_eq!(items.len(), 2);
        assert!(items[0].kind == Kind::Track && items[0].by == "Miles Davis");
        assert!(items[1].kind == Kind::Playlist && items[1].by == "ada" && items[1].id == "p");
        let more = parse_items(&json!({"items": [
            {"kind": "album", "uri": "spotify:album:a", "id": "a", "name": "Kind of Blue", "artists": "Miles Davis", "album": "1959"},
            {"kind": "artist", "uri": "spotify:artist:r", "id": "r", "name": "Miles Davis"},
            {"kind": "track", "uri": "spotify:track:t", "name": "So What", "artists": "Miles Davis", "album_id": "a", "artist_id": "r"},
        ]}));
        assert_eq!(described(&more[0]), "album by Miles Davis");
        assert_eq!(described(&more[1]), "artist");
        assert_eq!((more[2].album_id.as_str(), more[2].artist_id.as_str()), ("a", "r"));
        assert_eq!(Source::Liked.further(50), Some(json!({"cmd": "liked", "offset": 50})));
        assert_eq!(Source::Album { id: "a".into(), name: "n".into(), uri: "u".into() }.further(50), None, "an album comes whole");
        assert_eq!(Source::Album { id: "a".into(), name: "n".into(), uri: "spotify:album:a".into() }.context(), Some("spotify:album:a".to_string()));
    }

    #[test]
    fn finds_the_line_being_sung() {
        let lyrics = Lyrics::parse(&json!({"type": "lyrics", "track": "spotify:track:t", "synced": true, "error": null,
            "lines": [{"ms": 1200, "text": "So what"}, {"ms": 5400, "text": ""}, {"ms": 9000, "text": "and again"}]}));
        assert_eq!(lyrics.lines.len(), 3);
        assert_eq!(lyrics.current(0), None, "before the first line");
        assert_eq!(lyrics.current(1200), Some(0));
        assert_eq!(lyrics.current(8999), Some(1));
        assert_eq!(lyrics.current(600_000), Some(2));
        let plain = Lyrics { synced: false, ..lyrics.clone() };
        assert_eq!(plain.current(5000), None, "untimed words have no line being sung");
        let none = Lyrics::parse(&json!({"type": "lyrics", "track": "t", "synced": false, "lines": [], "error": "Spotify has no words for this track."}));
        assert!(none.lines.is_empty() && none.error.is_some());
    }

    #[test]
    fn a_playing_track_moves_on_but_not_past_its_end() {
        let mut now = Now::parse(&json!({"playing": true, "position_ms": 1000,
            "track": {"uri": "u", "name": "n", "duration_ms": 1200}}));
        now.at = Some(Instant::now() - Duration::from_secs(5));
        assert_eq!(now.position_ms(), 1200);
    }

    #[test]
    fn formats_times_and_names_lists() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(61_000), "1:01");
        assert_eq!(clock(562_000), "9:22");
        assert_eq!(Source::Liked.context().as_deref(), Some("liked"));
        assert_eq!(Source::Search("x".into()).context(), None);
        assert_eq!(Source::Playlist { id: "p".into(), name: "n".into(), uri: "spotify:playlist:p".into() }.key(), "tracks:p");
    }
}
