//! The DEs-UI music service: signs in to Spotify, plays through librespot, and serves
//! the shell's music player over a Unix socket.
//!
//! It is a program of its own so that music keeps playing when the shell restarts.
//! The shell speaks to it in lines of JSON (see `protocol`); `des-music send` does the
//! same from a command line, which is how the keyboard's media keys reach it.

mod protocol;
mod web;

use librespot_connect::{ConnectConfig, LoadRequest, LoadRequestOptions, PlayingTrack, Spirc};
use librespot_core::{
    authentication::Credentials, cache::Cache, config::SessionConfig, session::Session,
};
use librespot_metadata::audio::UniqueFields;
use librespot_playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{self, MixerConfig},
    player::{Player, PlayerEvent},
};
use protocol::{Request, State, Track};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

/// What Spotify is asked to allow: playing here, and reading the user's library.
const SCOPES: [&str; 5] = [
    "streaming",
    "user-read-private",
    "user-library-read",
    "playlist-read-private",
    "playlist-read-collaborative",
];
/// Where the browser hands the sign-in back to this program.
const REDIRECT: &str = "http://127.0.0.1:8898/login";
/// The program that turns raw samples on its input into sound, through PipeWire.
const OUTPUT: &str = "pw-cat --playback --raw --format s16 --rate 44100 --channels 2 -";

const USAGE: &str = "\
Usage: des-music [COMMAND]

  (none)         Run the music service
  login          Sign in to Spotify, in your browser; needed once
  library [ID]   Connect your library, with the client ID of your own Spotify app
  logout         Forget the sign-in
  send <WORD>    Tell a running service to: toggle, next, prev";

fn cache_dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("des-ui/music")
}

fn open_cache() -> Result<Cache, String> {
    let dir = cache_dir();
    // Credentials and the volume are kept; audio is not, to spare the disk.
    Cache::new(Some(&dir), Some(&dir), None, None).map_err(|e| format!("cannot use {}: {e}", dir.display()))
}

/// Signs in through the browser and keeps the result for the service to use.
async fn login() -> Result<(), String> {
    let cache = open_cache()?;
    let config = SessionConfig::default();
    println!("Opening Spotify's sign-in page in your browser…");
    let client = librespot_oauth::OAuthClientBuilder::new(&config.client_id, REDIRECT, SCOPES.to_vec())
        .open_in_browser()
        .with_custom_message("Signed in. You can close this page and go back to DEs-UI.")
        .build()
        .map_err(|e| format!("cannot start the sign-in: {e}"))?;
    let token = client
        .get_access_token_async()
        .await
        .map_err(|e| format!("the sign-in did not complete: {e}"))?;
    // Connecting once turns the short-lived token into credentials that last.
    let session = Session::new(config, Some(cache));
    session
        .connect(Credentials::with_access_token(token.access_token), true)
        .await
        .map_err(|e| format!("Spotify did not accept the sign-in: {e}"))?;
    println!("Signed in as {}.", session.username());
    Ok(())
}

/// The cover to show for a track: the one closest to what the player draws.
fn pick_cover(covers: &[librespot_metadata::audio::item::CoverImage]) -> Option<String> {
    covers
        .iter()
        .min_by_key(|cover| (cover.width - 300).abs())
        .map(|cover| cover.url.clone())
}

/// The name this player goes by among the user's devices: forty letters and digits,
/// the same at every start, so that it can be told apart from the others when
/// Spotify lists them. A service being tried out beside the real one gets its own.
fn device_id() -> String {
    let fresh = || {
        let mut bytes = [0u8; 20];
        let read = std::fs::File::open("/dev/urandom").and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes));
        if read.is_err() {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
            bytes[..16].copy_from_slice(&nanos.to_le_bytes());
            bytes[16..].copy_from_slice(&std::process::id().to_le_bytes());
        }
        bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
    };
    if std::env::var_os("DES_MUSIC_SOCKET").is_some() {
        return fresh();
    }
    let path = cache_dir().join("device-id");
    let kept = std::fs::read_to_string(&path).ok().map(|id| id.trim().to_string());
    match kept.filter(|id| id.len() == 40 && id.chars().all(|c| c.is_ascii_hexdigit())) {
        Some(id) => id,
        None => {
            let id = fresh();
            let _ = std::fs::write(&path, &id);
            id
        }
    }
}

struct Service {
    spirc: Spirc,
    session: Session,
    state: Mutex<(State, Instant)>,
    /// What another device is playing, as last heard, while one is: this then is what
    /// clients are shown, and what the controls act on.
    remote: Mutex<Option<(State, Instant)>>,
    /// The track last asked about, and whether it is among the liked songs.
    liked: Mutex<(String, Option<bool>)>,
    /// Woken to look at once at what is playing elsewhere.
    look: tokio::sync::Notify,
    updates: broadcast::Sender<String>,
}

/// How often what other devices are playing is looked at.
const LOOK_EVERY: Duration = Duration::from_secs(5);

pub const NO_LIKING: &str = "Liking needs one more approval from Spotify.";

impl Service {
    /// The state as of now, with the position moved on by the time since it was set.
    fn snapshot(&self) -> State {
        let (mut now, since) = match &*self.remote.lock().unwrap() {
            Some((state, since)) => (state.clone(), *since),
            None => {
                let guard = self.state.lock().unwrap();
                (guard.0.clone(), guard.1)
            }
        };
        if now.playing {
            let length = now.track.as_ref().map_or(u32::MAX, |track| track.duration_ms);
            now.position_ms = now.position_ms.saturating_add(since.elapsed().as_millis() as u32).min(length);
        }
        let liked = self.liked.lock().unwrap();
        now.liked = liked.1.filter(|_| now.track.as_ref().is_some_and(|track| track.uri == liked.0));
        now
    }

    /// Whether another device is the one playing.
    fn elsewhere(&self) -> bool {
        self.remote.lock().unwrap().is_some()
    }

    /// Changes what is shown of the other device at once, ahead of hearing it confirmed.
    fn update_remote(&self, change: impl FnOnce(&mut State)) {
        if let Some((state, since)) = self.remote.lock().unwrap().as_mut() {
            if state.playing {
                state.position_ms = state.position_ms.saturating_add(since.elapsed().as_millis() as u32);
            }
            *since = Instant::now();
            change(state);
        }
        self.publish();
    }

    /// Tells the device that is playing something, through the Web API. What goes
    /// wrong is said to the clients.
    async fn tell(&self, method: http::Method, path: &str, body: Option<serde_json::Value>) {
        if let Err(why) = web::send(&self.session, method, path, body).await {
            let _ = self.updates.send(protocol::problem_line(&why));
        }
        self.look.notify_one();
    }

    /// Finds out whether the track playing is liked, when the track changes.
    async fn check_liked(&self) {
        let Some(uri) = self.snapshot().track.map(|track| track.uri) else { return };
        if self.liked.lock().unwrap().0 == uri {
            return;
        }
        *self.liked.lock().unwrap() = (uri.clone(), None);
        let liked = web::is_liked(&self.session, &uri).await;
        let mut held = self.liked.lock().unwrap();
        if held.0 == uri {
            held.1 = liked;
            drop(held);
            self.publish();
        }
    }

    /// Looks at what is playing on whichever device, and takes it up when that is
    /// not this one.
    async fn look_elsewhere(&self) -> Result<(), String> {
        let found = web::player(&self.session).await?;
        let here = self.session.device_id().to_string();
        let remote = found.filter(|(_, device)| *device != here).map(|(state, _)| (state, Instant::now()));
        let changed = {
            let mut held = self.remote.lock().unwrap();
            let changed = held.as_ref().map(|(state, _)| state) != remote.as_ref().map(|(state, _)| state) || remote.is_some();
            *held = remote;
            changed
        };
        if changed {
            self.publish();
        }
        self.check_liked().await;
        Ok(())
    }

    fn publish(&self) {
        let _ = self.updates.send(self.snapshot().to_line());
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        {
            let mut guard = self.state.lock().unwrap();
            // Bring the position up to date before changing anything that it depends on.
            if guard.0.playing {
                guard.0.position_ms = guard.0.position_ms.saturating_add(guard.1.elapsed().as_millis() as u32);
            }
            guard.1 = Instant::now();
            change(&mut guard.0);
        }
        self.publish();
    }

    fn on_event(&self, event: PlayerEvent) {
        match event {
            PlayerEvent::TrackChanged { audio_item } => {
                let (artists, album) = match &audio_item.unique_fields {
                    UniqueFields::Track { artists, album, .. } => {
                        (artists.iter().map(|artist| artist.name.clone()).collect::<Vec<_>>().join(", "), album.clone())
                    }
                    UniqueFields::Episode { show_name, .. } => (show_name.clone(), String::new()),
                    UniqueFields::Local { artists, album, .. } => {
                        (artists.clone().unwrap_or_default(), album.clone().unwrap_or_default())
                    }
                };
                let track = Track {
                    uri: audio_item.uri.clone(),
                    name: audio_item.name.clone(),
                    artists,
                    album,
                    duration_ms: audio_item.duration_ms,
                    cover: pick_cover(&audio_item.covers),
                };
                self.update(|state| {
                    state.track = Some(track);
                    state.position_ms = 0;
                });
            }
            PlayerEvent::Playing { position_ms, .. } => {
                // Playing here: whatever another device was doing is over.
                *self.remote.lock().unwrap() = None;
                self.look.notify_one();
                self.update(|state| {
                    state.playing = true;
                    state.position_ms = position_ms;
                })
            }
            PlayerEvent::RepeatChanged { context, track } => self.update(|state| {
                state.repeat = if track { "one" } else if context { "all" } else { "off" }.to_string();
            }),
            PlayerEvent::Paused { position_ms, .. } => self.update(|state| {
                state.playing = false;
                state.position_ms = position_ms;
            }),
            PlayerEvent::Seeked { position_ms, .. }
            | PlayerEvent::PositionCorrection { position_ms, .. }
            | PlayerEvent::PositionChanged { position_ms, .. } => self.update(|state| state.position_ms = position_ms),
            PlayerEvent::Stopped { .. } => self.update(|state| state.playing = false),
            PlayerEvent::VolumeChanged { volume } => {
                self.update(|state| state.volume = (volume as u32 * 100 / u16::MAX as u32) as u8)
            }
            PlayerEvent::ShuffleChanged { shuffle } => self.update(|state| state.shuffle = shuffle),
            _ => {}
        }
    }

    /// Carries out one request. Returns what to send back to the one who asked.
    async fn handle(&self, request: Request) -> Option<String> {
        let done = |outcome: Result<(), librespot_core::Error>| {
            if let Err(e) = outcome {
                eprintln!("des-music: {e}");
            }
            None
        };
        use http::Method;
        // While another device plays, the controls are for it.
        if self.elsewhere() {
            match &request {
                Request::Toggle => {
                    let playing = self.snapshot().playing;
                    self.update_remote(|state| state.playing = !playing);
                    self.tell(Method::PUT, if playing { "/me/player/pause" } else { "/me/player/play" }, None).await;
                    return None;
                }
                Request::Next => {
                    self.tell(Method::POST, "/me/player/next", None).await;
                    return None;
                }
                Request::Previous => {
                    self.tell(Method::POST, "/me/player/previous", None).await;
                    return None;
                }
                Request::Seek(ms) => {
                    let ms = *ms;
                    self.update_remote(|state| state.position_ms = ms);
                    self.tell(Method::PUT, &format!("/me/player/seek?position_ms={ms}"), None).await;
                    return None;
                }
                Request::Volume(percent) => {
                    let percent = (*percent).min(100);
                    self.update_remote(|state| state.volume = percent);
                    self.tell(Method::PUT, &format!("/me/player/volume?volume_percent={percent}"), None).await;
                    return None;
                }
                Request::Shuffle(on) => {
                    let on = *on;
                    self.update_remote(|state| state.shuffle = on);
                    self.tell(Method::PUT, &format!("/me/player/shuffle?state={on}"), None).await;
                    return None;
                }
                Request::Repeat(mode) => {
                    let word = match mode.as_str() {
                        "all" => "context",
                        "one" => "track",
                        _ => "off",
                    };
                    let mode = mode.clone();
                    self.update_remote(|state| state.repeat = mode);
                    self.tell(Method::PUT, &format!("/me/player/repeat?state={word}"), None).await;
                    return None;
                }
                Request::Play { context, track } => {
                    let context = context.as_ref().map(|context| match context.as_str() {
                        "liked" => format!("spotify:user:{}:collection", self.session.username()),
                        other => other.to_string(),
                    });
                    let body = match (context, track) {
                        (Some(context), Some(track)) => json!({ "context_uri": context, "offset": { "uri": track } }),
                        (Some(context), None) => json!({ "context_uri": context }),
                        (None, Some(track)) => json!({ "uris": [track] }),
                        (None, None) => return None,
                    };
                    self.tell(Method::PUT, "/me/player/play", Some(body)).await;
                    return None;
                }
                _ => {}
            }
        }
        match request {
            Request::State => Some(self.snapshot().to_line()),
            Request::Repeat(mode) => {
                let _ = self.spirc.repeat(mode == "all");
                done(self.spirc.repeat_track(mode == "one"))
            }
            Request::Like(on) => {
                let Some(uri) = self.snapshot().track.map(|track| track.uri) else { return None };
                match web::set_liked(&self.session, &uri, on).await {
                    Ok(()) => {
                        *self.liked.lock().unwrap() = (uri, Some(on));
                        self.publish();
                        None
                    }
                    Err(why) if ["401", "403", "scope"].iter().any(|sign| why.contains(sign)) => Some(protocol::problem_line(NO_LIKING)),
                    Err(why) => Some(protocol::problem_line(&why)),
                }
            }
            Request::Lyrics(track) => Some(web::lyrics(&self.session, &track).await),
            Request::Devices => Some(web::devices(&self.session, self.session.device_id()).await),
            Request::Transfer(device) => {
                self.tell(Method::PUT, "/me/player", Some(json!({ "device_ids": [device], "play": true }))).await;
                // The devices take a moment to agree on who is playing.
                tokio::time::sleep(Duration::from_millis(900)).await;
                let _ = self.look_elsewhere().await;
                Some(web::devices(&self.session, self.session.device_id()).await)
            }
            Request::Toggle => done(self.spirc.play_pause()),
            Request::Next => done(self.spirc.next()),
            Request::Previous => done(self.spirc.prev()),
            Request::Seek(ms) => done(self.spirc.set_position_ms(ms)),
            Request::Volume(percent) => {
                done(self.spirc.set_volume((percent.min(100) as u32 * u16::MAX as u32 / 100) as u16))
            }
            Request::Shuffle(on) => done(self.spirc.shuffle(on)),
            Request::Play { context, track } => {
                let options = LoadRequestOptions {
                    start_playing: true,
                    playing_track: track.clone().filter(|_| context.is_some()).map(PlayingTrack::Uri),
                    ..Default::default()
                };
                let load = match (context, track) {
                    // The user's liked songs are a list only they have a name for.
                    (Some(context), _) if context == "liked" => LoadRequest::from_context_uri(
                        format!("spotify:user:{}:collection", self.session.username()),
                        options,
                    ),
                    (Some(context), _) => LoadRequest::from_context_uri(context, options),
                    (None, Some(track)) => LoadRequest::from_tracks(vec![track], options),
                    (None, None) => return None,
                };
                // Take over from whichever device was playing, then start.
                let _ = self.spirc.activate();
                done(self.spirc.load(load))
            }
            Request::Search(query) => Some(web::search(&self.session, &query).await),
            Request::Playlists => Some(web::playlists(&self.session).await),
            Request::Tracks { playlist, offset } => Some(web::tracks(&self.session, &playlist, offset).await),
            Request::Liked { offset } => Some(web::liked(&self.session, offset).await),
            Request::Albums { offset } => Some(web::albums(&self.session, offset).await),
            Request::Album(id) => Some(web::album(&self.session, &id).await),
            Request::Artist(id) => Some(web::artist(&self.session, &id).await),
            Request::Queue => Some(web::queue(&self.session).await),
            Request::Enqueue(track) => Some(web::enqueue(&self.session, &track).await),
        }
    }
}

async fn serve_client(service: Arc<Service>, stream: UnixStream) {
    let (reader, mut writer) = stream.into_split();
    let mut updates = service.updates.subscribe();
    let mut lines = BufReader::new(reader).lines();
    if writer.write_all(service.snapshot().to_line().as_bytes()).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                let Some(request) = Request::parse(&line) else { continue };
                if let Some(reply) = service.handle(request).await {
                    if writer.write_all(reply.as_bytes()).await.is_err() {
                        break;
                    }
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(line) => {
                        if writer.write_all(line.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    // Fell behind: the next update carries everything anyway.
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn serve() -> Result<(), String> {
    let path = protocol::socket_path();
    // One service at a time: a second would take the socket from under the first.
    if UnixStream::connect(&path).await.is_ok() {
        return Err("the music service is already running".to_string());
    }
    let cache = open_cache()?;
    let credentials = cache.credentials().ok_or("not signed in: run `des-music login` first")?;
    let session = Session::new(SessionConfig { device_id: device_id(), ..Default::default() }, Some(cache));

    let mixer_builder = mixer::find(None).ok_or("no volume control is built in")?;
    let mixer = mixer_builder(MixerConfig::default()).map_err(|e| format!("cannot set up the volume: {e}"))?;
    let sink_builder = audio_backend::find(Some("subprocess".to_string())).ok_or("no audio output is built in")?;
    let player = Player::new(PlayerConfig::default(), session.clone(), mixer.get_soft_volume(), move || {
        sink_builder(Some(OUTPUT.to_string()), AudioFormat::S16)
    });
    let mut events = player.get_player_event_channel();

    let connect = ConnectConfig {
        name: "DEs-UI".to_string(),
        ..Default::default()
    };
    let (spirc, spirc_task) = Spirc::new(connect, session.clone(), credentials, player, mixer)
        .await
        .map_err(|e| format!("cannot connect to Spotify: {e}"))?;
    eprintln!("des-music: signed in as {}", session.username());

    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|e| format!("cannot listen on {}: {e}", path.display()))?;
    let (updates, _) = broadcast::channel(32);
    let service = Arc::new(Service {
        spirc,
        session,
        state: Mutex::new((State::default(), Instant::now())),
        remote: Mutex::new(None),
        liked: Mutex::new((String::new(), None)),
        look: tokio::sync::Notify::new(),
        updates,
    });

    // What other devices are playing is asked for now and then, while anyone is
    // there to be shown it.
    let for_looking = service.clone();
    tokio::spawn(async move {
        let mut wait = LOOK_EVERY;
        loop {
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                // Something was just changed: give it a moment to take, then look.
                () = for_looking.look.notified() => tokio::time::sleep(Duration::from_millis(700)).await,
            }
            if for_looking.updates.receiver_count() == 0 {
                continue;
            }
            // Without a library connection, or with Spotify out of reach, ask less often.
            wait = if for_looking.look_elsewhere().await.is_ok() { LOOK_EVERY } else { LOOK_EVERY * 6 };
        }
    });

    let for_events = service.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            for_events.on_event(event);
        }
    });
    let for_clients = service.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve_client(for_clients.clone(), stream));
        }
    });

    // The service lasts as long as its connection to Spotify, or until told to stop.
    tokio::select! {
        () = spirc_task => eprintln!("des-music: the connection to Spotify ended"),
        _ = tokio::signal::ctrl_c() => {}
    }
    let _ = service.spirc.shutdown();
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// Sends one word to a running service, for scripts and the media keys.
async fn send(word: &str) -> Result<(), String> {
    let request = match word {
        "toggle" | "next" | "prev" => json!({ "cmd": word }),
        other => return Err(format!("unknown word '{other}'\n\n{USAGE}")),
    };
    let mut stream = UnixStream::connect(protocol::socket_path())
        .await
        .map_err(|_| "the music service is not running".to_string())?;
    stream
        .write_all(format!("{request}\n").as_bytes())
        .await
        .map_err(|e| e.to_string())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => serve().await,
        ["login"] => login().await,
        ["library", client] => connect_library(client).await,
        // Again, as the app already connected: for when more needs approving.
        ["library"] => match saved_client() {
            Some(client) => connect_library(&client).await,
            None => Err("give the client ID of your Spotify app: des-music library <ID>".to_string()),
        },
        ["logout"] => {
            let _ = std::fs::remove_file(web_token_file());
            std::fs::remove_file(cache_dir().join("credentials.json"))
                .map_err(|e| format!("there was no sign-in to forget: {e}"))
        }
        ["send", word] => send(word).await,
        ["-h" | "--help"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unrecognised arguments\n\n{USAGE}")),
    };
    if let Err(why) = outcome {
        eprintln!("des-music: {why}");
        // Lets the shell tell "not signed in" from other failures.
        std::process::exit(if why.starts_with("not signed in") { 3 } else { 1 });
    }
}

/// Where the sign-in's own token is kept: it is what reads the library.
fn web_token_file() -> PathBuf {
    cache_dir().join("web-token.json")
}

/// What the Web API is asked to allow: reading the user's library and liking tracks
/// in it, reading and steering what plays on any device, and adding to what plays next.
const LIBRARY_SCOPES: [&str; 7] = [
    "user-library-modify",
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-private",
    "user-library-read",
    "playlist-read-private",
    "playlist-read-collaborative",
];

/// The client ID the library was last connected with.
fn saved_client() -> Option<String> {
    let text = std::fs::read_to_string(web_token_file()).ok()?;
    serde_json::from_str::<serde_json::Value>(&text).ok()?["client"].as_str().map(str::to_string)
}

fn save_web_token(client: &str, token: &librespot_oauth::OAuthToken) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let left = token.expires_at.saturating_duration_since(Instant::now());
    let expires = (SystemTime::now() + left).duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let text = json!({ "client": client, "access": token.access_token, "refresh": token.refresh_token, "expires": expires })
        .to_string();
    let path = web_token_file();
    // Written under another name first, readable by the user alone.
    let fresh = path.with_extension("new");
    let _ = std::fs::remove_file(&fresh);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&fresh)
        .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()))
        .and_then(|()| std::fs::rename(&fresh, &path))
        .map_err(|e| format!("cannot keep the sign-in in {}: {e}", path.display()))
}

/// Signs in to the Web API as the user's own Spotify app. Spotify turns away library
/// requests made in the player's name, so reading the library needs an app of the
/// user's own; playing does not.
async fn connect_library(client: &str) -> Result<(), String> {
    if client.len() != 32 || !client.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("a client ID is 32 letters and digits, from your app's page on developer.spotify.com".to_string());
    }
    std::fs::create_dir_all(cache_dir()).map_err(|e| format!("cannot use {}: {e}", cache_dir().display()))?;
    println!("Opening Spotify's page in your browser, to let your app read your library…");
    let token = librespot_oauth::OAuthClientBuilder::new(client, REDIRECT, LIBRARY_SCOPES.to_vec())
        .open_in_browser()
        .with_custom_message("Library connected. You can close this page and go back to DEs-UI.")
        .build()
        .map_err(|e| format!("cannot start the sign-in: {e}"))?
        .get_access_token_async()
        .await
        .map_err(|e| format!("the sign-in did not complete: {e}"))?;
    save_web_token(client, &token)?;
    println!("Library connected.");
    Ok(())
}

pub const NO_QUEUE: &str = "The queue needs one more approval from Spotify. Run `des-music library` again.";

pub const NO_LIBRARY: &str = "Your library is not connected yet. Run `des-music library <client ID>` once; the README says where the ID comes from.";

/// A token for the Web API: the one from `library`, renewed when it runs out.
pub async fn web_token() -> Result<String, String> {
    let saved = std::fs::read_to_string(web_token_file())
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .ok_or(NO_LIBRARY)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Some(access) = saved["access"].as_str().filter(|_| saved["expires"].as_u64().unwrap_or(0) > now + 60) {
        return Ok(access.to_string());
    }
    let (Some(client), Some(refresh)) = (saved["client"].as_str(), saved["refresh"].as_str()) else {
        return Err(NO_LIBRARY.to_string());
    };
    let token = librespot_oauth::OAuthClientBuilder::new(client, REDIRECT, LIBRARY_SCOPES.to_vec())
        .build()
        .map_err(|e| format!("cannot renew the library connection: {e}"))?
        .refresh_token_async(refresh)
        .await
        .map_err(|e| format!("Spotify would not renew the library connection ({e}). Run `des-music library` again."))?;
    // Spotify does not always send a new renewal token; the old one then still stands.
    let mut token = token;
    if token.refresh_token.is_empty() {
        token.refresh_token = refresh.to_string();
    }
    save_web_token(client, &token)?;
    Ok(token.access_token)
}
