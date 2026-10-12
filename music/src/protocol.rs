//! The lines of JSON the music service and its clients exchange.
//!
//! A client sends `{"cmd": ...}`. The service sends `{"type":"state", ...}` whenever
//! what is playing changes, and `{"type":"list", ...}` in answer to a request for one.

use serde_json::{Value, json};
use std::path::PathBuf;

/// Where the service listens. `DES_MUSIC_SOCKET` names another place, for trying
/// things out beside a service that is in use.
pub fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("DES_MUSIC_SOCKET") {
        return PathBuf::from(path);
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("des-music.sock")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub uri: String,
    pub name: String,
    pub artists: String,
    pub album: String,
    pub duration_ms: u32,
    /// Address of the cover picture.
    pub cover: Option<String>,
}

impl Track {
    pub fn to_json(&self) -> Value {
        json!({
            "uri": self.uri,
            "name": self.name,
            "artists": self.artists,
            "album": self.album,
            "duration_ms": self.duration_ms,
            "cover": self.cover,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub track: Option<Track>,
    pub playing: bool,
    pub position_ms: u32,
    /// Percent.
    pub volume: u8,
    pub shuffle: bool,
    /// `off`, `all` (the list starts over) or `one` (the track does).
    pub repeat: String,
    /// The device this is playing on, when it is not this one.
    pub device: Option<String>,
    /// Whether the track is among the liked songs, once known.
    pub liked: Option<bool>,
}

impl State {
    pub fn to_line(&self) -> String {
        let state = json!({
            "type": "state",
            "track": self.track.as_ref().map(Track::to_json),
            "playing": self.playing,
            "position_ms": self.position_ms,
            "volume": self.volume,
            "shuffle": self.shuffle,
            "repeat": if self.repeat.is_empty() { "off" } else { &self.repeat },
            "device": self.device,
            "liked": self.liked,
        });
        format!("{state}\n")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    State,
    Toggle,
    Next,
    Previous,
    Seek(u32),
    Volume(u8),
    Shuffle(bool),
    /// Play a list (a playlist's address, or `liked`) from a track, or one track alone.
    Play { context: Option<String>, track: Option<String> },
    Search(String),
    Playlists,
    /// The tracks of the playlist with this id, from the one at `offset` on.
    Tracks { playlist: String, offset: u32 },
    /// The liked songs, from the one at `offset` on.
    Liked { offset: u32 },
    /// The albums saved to the library, from the one at `offset` on.
    Albums { offset: u32 },
    /// The tracks of the album with this id.
    Album(String),
    /// The albums of the artist with this id.
    Artist(String),
    /// What plays next.
    Queue,
    /// Add the track with this address to what plays next.
    Enqueue(String),
    /// `off`, `all` or `one`.
    Repeat(String),
    /// Put the track playing among the liked songs, or take it out.
    Like(bool),
    /// The words of the track with this address.
    Lyrics(String),
    /// The devices that could play.
    Devices,
    /// Carry on playing on the device with this id.
    Transfer(String),
}

impl Request {
    pub fn parse(line: &str) -> Option<Request> {
        let value: Value = serde_json::from_str(line).ok()?;
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        // Ids go into addresses as they are, so only what an id is made of is let through.
        let id = |key: &str| text(key).filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()));
        let offset = value.get("offset").and_then(Value::as_u64).unwrap_or(0).min(100_000) as u32;
        Some(match value.get("cmd")?.as_str()? {
            "state" => Request::State,
            "toggle" => Request::Toggle,
            "next" => Request::Next,
            "prev" => Request::Previous,
            "seek" => Request::Seek(value.get("ms")?.as_u64()?.try_into().ok()?),
            "volume" => Request::Volume(value.get("percent")?.as_u64()?.min(100) as u8),
            "shuffle" => Request::Shuffle(value.get("on")?.as_bool()?),
            "play" => {
                let (context, track) = (text("context"), text("track"));
                if context.is_none() && track.is_none() {
                    return None;
                }
                Request::Play { context, track }
            }
            "search" => Request::Search(text("q").filter(|q| !q.trim().is_empty())?),
            "playlists" => Request::Playlists,
            "tracks" => Request::Tracks { playlist: text("playlist")?, offset },
            "liked" => Request::Liked { offset },
            "albums" => Request::Albums { offset },
            "album" => Request::Album(id("album")?),
            "artist" => Request::Artist(id("artist")?),
            "queue" => Request::Queue,
            "repeat" => Request::Repeat(text("mode").filter(|mode| matches!(mode.as_str(), "off" | "all" | "one"))?),
            "like" => Request::Like(value.get("on")?.as_bool()?),
            "lyrics" => Request::Lyrics(text("track").filter(|uri| uri.starts_with("spotify:track:"))?),
            "devices" => Request::Devices,
            "transfer" => Request::Transfer(id("device")?),
            "enqueue" => Request::Enqueue(text("track").filter(|uri| uri.starts_with("spotify:track:"))?),
            _ => return None,
        })
    }
}

/// A track's words: each line with the moment it is sung, in milliseconds, when the
/// lines are `synced` to the music; and why there are none, when there are none.
pub fn lyrics_line(track: &str, synced: bool, lines: Vec<Value>, error: Option<String>) -> String {
    format!("{}\n", json!({ "type": "lyrics", "track": track, "synced": synced, "lines": lines, "error": error }))
}

/// Something that went wrong with what a client asked, in words for the user.
pub fn problem_line(text: &str) -> String {
    format!("{}\n", json!({ "type": "problem", "text": text }))
}

/// A list answered to a client: `kind` says of what, `key` which one (the search text
/// or the playlist's id), and `error` why it is empty when something went wrong.
pub fn list_line(kind: &str, key: &str, items: Vec<Value>, error: Option<String>) -> String {
    page_line(kind, key, items, error, 0, false)
}

/// A part of a list: the items from `offset` on, and whether there are `more` after them.
pub fn page_line(kind: &str, key: &str, items: Vec<Value>, error: Option<String>, offset: u32, more: bool) -> String {
    format!("{}\n", json!({ "type": "list", "kind": kind, "key": key, "items": items, "error": error, "offset": offset, "more": more }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests() {
        assert_eq!(Request::parse(r#"{"cmd":"toggle"}"#), Some(Request::Toggle));
        assert_eq!(Request::parse(r#"{"cmd":"seek","ms":61000}"#), Some(Request::Seek(61000)));
        assert_eq!(Request::parse(r#"{"cmd":"volume","percent":250}"#), Some(Request::Volume(100)));
        assert_eq!(Request::parse(r#"{"cmd":"shuffle","on":true}"#), Some(Request::Shuffle(true)));
        assert_eq!(
            Request::parse(r#"{"cmd":"play","context":"spotify:playlist:abc","track":"spotify:track:x"}"#),
            Some(Request::Play { context: Some("spotify:playlist:abc".into()), track: Some("spotify:track:x".into()) })
        );
        assert_eq!(Request::parse(r#"{"cmd":"search","q":"miles davis"}"#), Some(Request::Search("miles davis".into())));
        assert_eq!(Request::parse(r#"{"cmd":"tracks","playlist":"abc"}"#), Some(Request::Tracks { playlist: "abc".into(), offset: 0 }));
        assert_eq!(Request::parse(r#"{"cmd":"tracks","playlist":"abc","offset":200}"#), Some(Request::Tracks { playlist: "abc".into(), offset: 200 }));
        assert_eq!(Request::parse(r#"{"cmd":"liked","offset":50}"#), Some(Request::Liked { offset: 50 }));
        assert_eq!(Request::parse(r#"{"cmd":"albums"}"#), Some(Request::Albums { offset: 0 }));
        assert_eq!(Request::parse(r#"{"cmd":"album","album":"4aawyAB9vmqN3uQ7FjRGTy"}"#), Some(Request::Album("4aawyAB9vmqN3uQ7FjRGTy".into())));
        assert_eq!(Request::parse(r#"{"cmd":"artist","artist":"0TnOYISbd1XYRBk9myaseg"}"#), Some(Request::Artist("0TnOYISbd1XYRBk9myaseg".into())));
        assert_eq!(Request::parse(r#"{"cmd":"album","album":"../me"}"#), None);
        assert_eq!(Request::parse(r#"{"cmd":"repeat","mode":"one"}"#), Some(Request::Repeat("one".into())));
        assert_eq!(Request::parse(r#"{"cmd":"repeat","mode":"twice"}"#), None);
        assert_eq!(Request::parse(r#"{"cmd":"like","on":false}"#), Some(Request::Like(false)));
        assert_eq!(Request::parse(r#"{"cmd":"devices"}"#), Some(Request::Devices));
        assert_eq!(Request::parse(r#"{"cmd":"lyrics","track":"spotify:track:x"}"#), Some(Request::Lyrics("spotify:track:x".into())));
        assert_eq!(Request::parse(r#"{"cmd":"lyrics","track":"spotify:album:x"}"#), None);
        assert_eq!(Request::parse(r#"{"cmd":"transfer","device":"0d1841b0976bae2a3a310dd74c0f3df354899bc8"}"#), Some(Request::Transfer("0d1841b0976bae2a3a310dd74c0f3df354899bc8".into())));
        assert_eq!(Request::parse(r#"{"cmd":"queue"}"#), Some(Request::Queue));
        assert_eq!(Request::parse(r#"{"cmd":"enqueue","track":"spotify:track:x"}"#), Some(Request::Enqueue("spotify:track:x".into())));
        assert_eq!(Request::parse(r#"{"cmd":"enqueue","track":"spotify:playlist:x"}"#), None);
    }

    #[test]
    fn rejects_what_it_cannot_act_on() {
        for line in ["", "toggle", r#"{"cmd":"dance"}"#, r#"{"cmd":"seek"}"#, r#"{"cmd":"seek","ms":-5}"#, r#"{"cmd":"play"}"#, r#"{"cmd":"search","q":"  "}"#, r#"{"cmd":"tracks"}"#] {
            assert_eq!(Request::parse(line), None, "{line}");
        }
    }

    #[test]
    fn state_is_one_line_of_json() {
        let state = State {
            track: Some(Track {
                uri: "spotify:track:x".into(),
                name: "So \"What\"\n".into(),
                artists: "Miles Davis".into(),
                album: "Kind of Blue".into(),
                duration_ms: 562000,
                cover: None,
            }),
            playing: true,
            position_ms: 1000,
            volume: 80,
            shuffle: false,
            repeat: String::new(),
            device: Some("Kitchen".into()),
            liked: Some(true),
        };
        let line = state.to_line();
        assert_eq!(line.matches('\n').count(), 1);
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["type"], "state");
        assert_eq!(value["track"]["name"], "So \"What\"\n");
        assert_eq!(value["volume"], 80);
        assert_eq!((value["repeat"].as_str(), value["device"].as_str(), value["liked"].as_bool()), (Some("off"), Some("Kitchen"), Some(true)));
        assert_eq!(serde_json::from_str::<Value>(&State::default().to_line()).unwrap()["track"], Value::Null);
    }
}
