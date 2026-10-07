//! Hyprland IPC — workspace state for the tab strip.
//!
//! Lives in khadi-core rather than khadi-bar because it is a system data
//! source like `metrics` or `net`, and carries no UI dependency. The bar is
//! its only consumer today.
//!
//! Hyprland exposes two sockets. `.socket.sock` answers queries; `.socket2.sock`
//! streams events. Polling the first on a timer would either lag a workspace
//! switch or waste a roundtrip every tick, so the model here is: block on the
//! event stream in a thread, and re-query only when an event says the
//! workspaces changed.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

/// One workspace, as `hyprctl -j workspaces` reports it. Unknown fields are
/// ignored: this is a rolling-release target and Hyprland adds keys.
#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub id: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub windows: u32,
}

/// Everything the tab strip needs, read as one consistent set.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub workspaces: Vec<Workspace>,
    pub active: i32,
    /// Something on the active workspace is fullscreen. The bar uses this to
    /// get out of the way: section 11's "the aesthetic must be dismissible".
    pub fullscreen: bool,
}

impl Snapshot {
    pub fn workspace(&self, id: i32) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }
}

fn signature() -> Option<String> {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()
}

/// Hyprland moved its sockets from `/tmp/hypr` to `$XDG_RUNTIME_DIR/hypr` in
/// 0.40. Try both, newest location first, so the bar works on either.
fn socket_dir(sig: &str) -> Option<PathBuf> {
    std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(|rt| PathBuf::from(rt).join("hypr").join(sig))
        .into_iter()
        .chain(std::iter::once(PathBuf::from("/tmp/hypr").join(sig)))
        .find(|p| p.is_dir())
}

fn socket(name: &str) -> Result<PathBuf> {
    let sig = signature().ok_or_else(|| anyhow!("HYPRLAND_INSTANCE_SIGNATURE is not set"))?;
    let dir = socket_dir(&sig).ok_or_else(|| anyhow!("no hypr socket directory for {sig}"))?;
    Ok(dir.join(name))
}

/// Whether we are inside a Hyprland session at all. The bar renders static
/// placeholders when we are not, which is the case when testing it on another
/// compositor.
pub fn available() -> bool {
    socket(".socket.sock").map(|p| p.exists()).unwrap_or(false)
}

/// One request on the command socket. Hyprland answers and closes, so the
/// connection is per-call by design, not by omission.
fn request(cmd: &str) -> Result<String> {
    let path = socket(".socket.sock")?;
    let mut s = UnixStream::connect(&path)
        .with_context(|| format!("connect {}", path.display()))?;
    s.write_all(cmd.as_bytes())?;
    s.flush()?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

pub fn snapshot() -> Result<Snapshot> {
    #[derive(Deserialize)]
    struct Active {
        id: i32,
        #[serde(default)]
        hasfullscreen: bool,
    }

    let raw = request("j/workspaces")?;
    let mut workspaces: Vec<Workspace> =
        serde_json::from_str(&raw).context("parse j/workspaces")?;
    // Hyprland returns them in creation order; the tab strip is positional.
    workspaces.sort_by_key(|w| w.id);

    let raw = request("j/activeworkspace")?;
    let active: Active = serde_json::from_str(&raw).context("parse j/activeworkspace")?;

    Ok(Snapshot { workspaces, active: active.id, fullscreen: active.hasfullscreen })
}

/// Events that change what the tab strip shows. Everything else on the stream
/// — `activewindow`, `submap`, monitor and layer traffic — would cost two
/// socket roundtrips to discover nothing had moved.
fn affects_workspaces(line: &str) -> bool {
    // Events are `name>>payload`; Hyprland ships `v2` variants of several with
    // richer payloads and emits both.
    let name = line.split_once(">>").map(|(n, _)| n).unwrap_or(line);
    let name = name.strip_suffix("v2").unwrap_or(name);
    matches!(
        name,
        "workspace"
            | "createworkspace"
            | "destroyworkspace"
            | "renameworkspace"
            | "moveworkspace"
            | "focusedmon"
            | "openwindow"
            | "closewindow"
            | "movewindow"
            | "activespecial"
            // Not a workspace move, but the bar hides on it.
            | "fullscreen"
    )
}

/// A background reader of the event socket.
///
/// The thread owns the blocking read and publishes the newest snapshot; the UI
/// thread collects it whenever it likes. Snapshots are not queued — a stale one
/// has no value — so a burst of events costs the consumer one update.
pub struct Watcher {
    latest: Arc<Mutex<Option<Snapshot>>>,
}

impl Watcher {
    pub fn spawn() -> Self {
        let latest = Arc::new(Mutex::new(None));
        let sink = Arc::clone(&latest);
        thread::Builder::new()
            .name("khadi-hypr".into())
            .spawn(move || watch(&sink))
            .expect("spawn hypr watcher");
        Self { latest }
    }

    /// The newest snapshot, if one arrived since the last call.
    pub fn take(&self) -> Option<Snapshot> {
        self.latest.lock().ok()?.take()
    }
}

fn publish(sink: &Arc<Mutex<Option<Snapshot>>>) {
    match snapshot() {
        Ok(s) => {
            if let Ok(mut slot) = sink.lock() {
                *slot = Some(s);
            }
        }
        // A failed query is not fatal: the compositor may be mid-restart, and
        // the next event will ask again.
        Err(e) => eprintln!("khadi: hypr snapshot failed: {e}"),
    }
}

fn watch(sink: &Arc<Mutex<Option<Snapshot>>>) {
    loop {
        if let Ok(path) = socket(".socket2.sock") {
            if let Ok(stream) = UnixStream::connect(&path) {
                // Publish on connect, so the first paint is real state rather
                // than whatever the bar was built with.
                publish(sink);
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    if affects_workspaces(&line) {
                        publish(sink);
                    }
                }
            }
        }
        // Either Hyprland is not up yet or it restarted and took the socket
        // with it. Retry instead of exiting: a bar that silently stops
        // tracking workspaces looks like a bug in the tabs.
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_json_parses_and_ignores_new_keys() {
        // Trimmed from real `hyprctl -j workspaces` output, plus a key that
        // does not exist yet — upstream adds them without warning.
        let raw = r#"[{"id":2,"name":"2","monitor":"eDP-1","windows":3,
                       "hasfullscreen":false,"somethingnew":42},
                      {"id":1,"name":"1","monitor":"eDP-1","windows":0}]"#;
        let mut got: Vec<Workspace> = serde_json::from_str(raw).unwrap();
        got.sort_by_key(|w| w.id);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, 1);
        assert_eq!(got[0].windows, 0);
        assert_eq!(got[1].windows, 3);
    }

    #[test]
    fn the_event_filter_keeps_workspace_traffic() {
        for line in [
            "workspace>>2",
            "workspacev2>>2,2",
            "createworkspace>>3",
            "destroyworkspacev2>>3,3",
            "openwindow>>8a1,1,foot,shell",
            "closewindow>>8a1",
            "focusedmonv2>>eDP-1,2",
            "activespecial>>,eDP-1",
            "fullscreen>>1",
        ] {
            assert!(affects_workspaces(line), "dropped {line}");
        }
    }

    #[test]
    fn the_event_filter_drops_the_chatty_ones() {
        // activewindow fires on every focus change inside one workspace, which
        // is most of the stream and never moves a tab.
        for line in [
            "activewindow>>foot,shell",
            "activewindowv2>>8a1",
            "submap>>resize",
            "monitoradded>>DP-2",
            "openlayer>>khadi-bar",
            "configreloaded",
        ] {
            assert!(!affects_workspaces(line), "kept {line}");
        }
    }

    /// Against the real compositor, because the unit tests above prove the
    /// parsing and the filter and nothing about whether the socket path, the
    /// query strings or the JSON shape still match live Hyprland. Ignored by
    /// default — there is no compositor in CI.
    ///
    ///     cargo test -p khadi-core hypr -- --ignored --nocapture
    #[test]
    #[ignore = "needs a running Hyprland session"]
    fn live_snapshot_against_a_real_compositor() {
        assert!(available(), "no Hyprland session to talk to");
        let s = snapshot().expect("snapshot");
        println!("active {} of {:?}", s.active, s.workspaces);
        assert!(!s.workspaces.is_empty(), "a session always has one workspace");
        assert!(
            s.workspace(s.active).is_some(),
            "the active workspace must appear in the list"
        );
    }

    /// The watcher publishes on connect, so this proves the event socket is
    /// where we think it is and the publish path reaches a consumer. What an
    /// event *triggers* is the same `publish`, and which events qualify is
    /// covered by the filter tests above.
    #[test]
    #[ignore = "needs a running Hyprland session"]
    fn the_watcher_connects_and_publishes() {
        let w = Watcher::spawn();
        let mut got = None;
        for _ in 0..40 {
            if let Some(s) = w.take() {
                got = Some(s);
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let s = got.expect("no snapshot within 2s of spawning the watcher");
        assert!(!s.workspaces.is_empty());
        // And nothing further, until something actually moves.
        thread::sleep(Duration::from_millis(200));
        assert!(w.take().is_none(), "published with no event to publish for");
    }

    #[test]
    fn snapshot_lookup_is_by_id_not_position() {
        let s = Snapshot {
            workspaces: vec![
                Workspace { id: 1, name: "1".into(), windows: 1 },
                Workspace { id: 4, name: "code".into(), windows: 2 },
            ],
            active: 4,
            fullscreen: false,
        };
        assert_eq!(s.workspace(4).unwrap().name, "code");
        assert!(s.workspace(2).is_none());
    }
}
