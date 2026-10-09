//! khadi-shell — eDEX's layout, as a Tauri app.
//!
//! The webview owns the screen; this owns the machine. Every number on the
//! display comes from `khadi-core`, which is the same crate the ratatui panels
//! and the GTK bar read — it was kept free of UI dependencies in Phase 2 for
//! exactly this reason, and nothing in it had to change to feed a browser.
//!
//! Commands are coarse: one call returns a whole panel's worth of state. A
//! chatty IPC boundary is the usual way a webview UI ends up janky, and the
//! panels refresh on a timer anyway.

mod greet;
mod pty;

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use khadi_core::{browse::Browser, disks::Filesystem, metrics::Metrics, net::Network, Theme};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub struct Sources {
    metrics: Mutex<Metrics>,
    /// Flips each `system` call. The process walk is most of a refresh's cost
    /// and the list changes far more slowly than the CPU graph beside it.
    tick: AtomicU64,
    net: Mutex<Network>,
    disks: Mutex<Filesystem>,
    browser: Mutex<Browser>,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            metrics: Mutex::new(Metrics::new()),
            tick: AtomicU64::new(0),
            net: Mutex::new(Network::new()),
            disks: Mutex::new(Filesystem::new()),
            browser: Mutex::new(Browser::new()),
        }
    }
}

// ---------------------------------------------------------------- the theme

/// The theme, flattened for CSS. `khadi-theme` already resolves the ramp and
/// writes `theme.toml`; re-deriving it here would be the second implementation
/// of one derivation, which is the thing Phase 2 set out to avoid.
#[derive(Serialize)]
pub struct ThemeOut {
    name: String,
    text: String,
    text_dim: String,
    text_muted: String,
    rule: String,
    rule_faint: String,
    ground: String,
    ramp: Vec<String>,
    mono: String,
}

fn hex(c: khadi_core::Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

#[tauri::command]
fn theme() -> Result<ThemeOut, String> {
    let t = Theme::load().map_err(|e| e.to_string())?;
    Ok(ThemeOut {
        name: t.name.clone(),
        text: hex(t.text),
        text_dim: hex(t.text_dim),
        text_muted: hex(t.text_muted),
        rule: hex(t.rule),
        rule_faint: hex(t.rule_faint),
        ground: hex(t.ground),
        ramp: t.ramp.iter().map(|c| hex(*c)).collect(),
        mono: t.mono.clone(),
    })
}

// --------------------------------------------------------------- the panels

#[derive(Serialize)]
pub struct Proc {
    pid: u32,
    name: String,
    cpu: f32,
    mem: u64,
}

#[derive(Serialize)]
pub struct SystemOut {
    /// Per-core history, newest last. eDEX draws one canvas per core PAIR.
    cores: Vec<Vec<f32>>,
    mem_used: u64,
    mem_total: u64,
    swap_used: u64,
    swap_total: u64,
    procs: Vec<Proc>,
    temp_c: Option<f32>,
    tasks: usize,
    uptime_secs: u64,
    vendor: String,
    model: String,
    chassis: String,
    cpu_model: String,
    cores_total: usize,
    host: String,
}

#[tauri::command]
fn system(src: State<'_, Sources>) -> Result<SystemOut, String> { system_of(&src) }
fn system_of(src: &Sources) -> Result<SystemOut, String> {
    let mut m = src.metrics.lock().map_err(|e| e.to_string())?;
    let n = src.tick.fetch_add(1, Ordering::Relaxed);
    m.refresh_with(n % 2 == 0);
    Ok(SystemOut {
        cores: m.per_core.iter().map(|h| h.iter().copied().collect()).collect(),
        mem_used: m.memory.used,
        mem_total: m.memory.total,
        swap_used: m.memory.swap_used,
        swap_total: m.memory.swap_total,
        procs: m
            .procs
            .iter()
            .map(|p| Proc { pid: p.pid, name: p.name.clone(), cpu: p.cpu, mem: p.mem })
            .collect(),
        temp_c: m.temp_c,
        tasks: m.tasks,
        uptime_secs: m.uptime.as_secs(),
        vendor: m.hardware.vendor.clone(),
        model: m.hardware.model.clone(),
        chassis: m.hardware.chassis.clone(),
        cpu_model: m.hardware.cpu_model.clone(),
        cores_total: m.hardware.cores,
        host: m.hardware.host.clone(),
    })
}

#[derive(Serialize)]
pub struct Peer {
    addr: String,
    count: usize,
}

#[derive(Serialize)]
pub struct NetworkOut {
    up: bool,
    iface: String,
    ipv4: String,
    gateway: String,
    dns: String,
    mac: String,
    mtu: String,
    rx_rate: f64,
    tx_rate: f64,
    rx_total: u64,
    tx_total: u64,
    rx_hist: Vec<f64>,
    tx_hist: Vec<f64>,
    established: usize,
    listening: usize,
    peers: Vec<Peer>,
    listeners: Vec<String>,
}

#[tauri::command]
fn network(src: State<'_, Sources>) -> Result<NetworkOut, String> { network_of(&src) }
fn network_of(src: &Sources) -> Result<NetworkOut, String> {
    let mut n = src.net.lock().map_err(|e| e.to_string())?;
    n.refresh();
    Ok(NetworkOut {
        up: n.iface.up,
        iface: n.iface.name.clone(),
        ipv4: n.iface.ipv4.clone(),
        gateway: n.iface.gateway.clone(),
        dns: n.iface.dns.clone(),
        mac: n.iface.mac.clone(),
        mtu: n.iface.mtu.clone(),
        rx_rate: n.rx_rate,
        tx_rate: n.tx_rate,
        rx_total: n.rx_total,
        tx_total: n.tx_total,
        rx_hist: n.rx_hist.iter().copied().collect(),
        tx_hist: n.tx_hist.iter().copied().collect(),
        established: n.sockets.established,
        listening: n.sockets.listening,
        peers: n
            .sockets
            .peers
            .iter()
            .map(|(a, c)| Peer { addr: a.clone(), count: *c })
            .collect(),
        listeners: n.sockets.listeners.clone(),
    })
}

#[derive(Serialize)]
pub struct FsEntry {
    name: String,
    kind: String,
    size: u64,
    hidden: bool,
}

#[derive(Serialize)]
pub struct FsOut {
    cwd: String,
    entries: Vec<FsEntry>,
    used: u64,
    total: u64,
    mount: String,
}

#[tauri::command]
fn filesystem(src: State<'_, Sources>) -> Result<FsOut, String> { filesystem_of(&src) }
fn filesystem_of(src: &Sources) -> Result<FsOut, String> {
    use khadi_core::browse::Kind;
    let mut b = src.browser.lock().map_err(|e| e.to_string())?;
    b.refresh();
    Ok(FsOut {
        cwd: b.cwd.to_string_lossy().to_string(),
        entries: b
            .entries
            .iter()
            .map(|e| FsEntry {
                name: e.name.clone(),
                kind: match e.kind {
                    Kind::Up => "up",
                    Kind::Dir => "dir",
                    Kind::File => "file",
                    Kind::Link => "link",
                    Kind::Other => "other",
                }
                .into(),
                size: e.size,
                hidden: e.hidden,
            })
            .collect(),
        used: b.used,
        total: b.total,
        mount: b.mount.clone(),
    })
}

#[derive(Serialize)]
pub struct Mount {
    path: String,
    used: u64,
    total: u64,
}

#[tauri::command]
fn mounts(src: State<'_, Sources>) -> Result<Vec<Mount>, String> { mounts_of(&src) }
fn mounts_of(src: &Sources) -> Result<Vec<Mount>, String> {
    let mut d = src.disks.lock().map_err(|e| e.to_string())?;
    d.refresh();
    Ok(d.mounts
        .iter()
        .map(|m| Mount { path: m.path.clone(), used: m.used, total: m.total })
        .collect())
}

// ------------------------------------------------------------ the workspaces
//
// The one thing `khadi-bar` carried that nothing else did. Its clock, CPU,
// memory and network readouts duplicated the side panels — its own module doc
// said so — and its angled tab strip was there because a cell grid cannot
// skew. A webview can, so the whole bar folds into this.

#[derive(Serialize)]
pub struct Workspace {
    id: i32,
    name: String,
    windows: u32,
    active: bool,
}

#[tauri::command]
fn workspaces() -> Result<Vec<Workspace>, String> {
    if !khadi_core::hypr::available() {
        return Ok(Vec::new());
    }
    let snap = khadi_core::hypr::snapshot().map_err(|e| e.to_string())?;
    Ok(snap
        .workspaces
        .iter()
        .map(|w| Workspace {
            id: w.id,
            name: w.name.clone(),
            windows: w.windows,
            active: w.id == snap.active,
        })
        .collect())
}

#[tauri::command]
fn workspace_goto(id: i32) -> Result<(), String> {
    khadi_core::hypr::goto_workspace(id).map_err(|e| e.to_string())
}

// ------------------------------------------------------------- the terminal

#[tauri::command]
fn pty_spawn(
    app: AppHandle,
    state: State<'_, pty::Ptys>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    pty::spawn(&app, &state, &id, cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
fn pty_write(state: State<'_, pty::Ptys>, id: String, data: String) -> Result<(), String> {
    pty::write(&state, &id, &data).map_err(|e| e.to_string())
}

#[tauri::command]
fn pty_resize(
    state: State<'_, pty::Ptys>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    pty::resize(&state, &id, cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
fn pty_kill(state: State<'_, pty::Ptys>, id: String) -> Result<(), String> {
    pty::kill(&state, &id).map_err(|e| e.to_string())
}


/// `khadi-shell --selftest` — ask the binary whether it can read the machine.
///
/// THE GATE USED TO HAVE THIS AND LOST IT. Its sharpest check was "on PATH is
/// not the same as working": a stale khadi-hud once passed 71/71 while three
/// of its four panels were dead, because nothing asked the binary to draw. The
/// shell cannot draw into a pipe, so after the rewrite the gate was down to
/// two checks — a window exists, a pty spawned — and a panel full of dashes
/// would have sailed through. The globe shipped upside down under exactly that
/// gap.
///
/// This runs every data command the webview calls, through the same code, and
/// reports one line each. It needs no compositor and no window.
pub fn selftest() -> i32 {
    let src = Sources::default();
    let mut bad = 0;

    macro_rules! check {
        ($name:literal, $body:expr) => {
            match $body {
                Ok(detail) => println!("ok   {:<12} {}", $name, detail),
                Err(e) => {
                    println!("FAIL {:<12} {}", $name, e);
                    bad += 1;
                }
            }
        };
    }

    check!("theme", theme().map(|t| format!("{} text={}", t.name, t.text)));
    check!("system", {
        // Twice: per-core history needs two samples before it means anything,
        // and the second call is the one that walks /proc.
        let _ = system_of(&src);
        std::thread::sleep(std::time::Duration::from_millis(300));
        system_of(&src).and_then(|s| {
            if s.cores.is_empty() {
                Err("no CPU cores".into())
            } else if s.mem_total == 0 {
                Err("no memory total".into())
            } else if s.procs.is_empty() {
                Err("no processes".into())
            } else {
                Ok(format!("{} cores, {} procs, {} MiB", s.cores.len(), s.procs.len(), s.mem_total / 1048576))
            }
        })
    });
    check!("network", network_of(&src).and_then(|n| {
        if n.rx_hist.is_empty() {
            Err("no traffic history".into())
        } else {
            Ok(format!("{} rx_hist={} est={}", if n.up { &n.iface } else { "offline" }, n.rx_hist.len(), n.established))
        }
    }));
    check!("filesystem", filesystem_of(&src).and_then(|f| {
        if f.entries.is_empty() {
            Err(format!("no entries in {}", f.cwd))
        } else if f.total == 0 {
            Err("statvfs gave no total".into())
        } else {
            Ok(format!("{} ({} entries)", f.cwd, f.entries.len()))
        }
    }));
    check!("mounts", mounts_of(&src).and_then(|m| {
        if m.is_empty() { Err("no mounts".into()) } else { Ok(format!("{} mounted", m.len())) }
    }));
    // Workspaces are the one thing that legitimately answers empty: no
    // compositor means no workspaces, which is not a failure of the shell.
    check!("workspaces", workspaces().map(|w| {
        if w.is_empty() { "none (no compositor)".to_string() } else { format!("{} live", w.len()) }
    }));

    if bad == 0 {
        println!("\nselftest: all data sources answered");
    } else {
        println!("\nselftest: {bad} source(s) failed");
    }
    bad
}

/// Greeter mode: a different window, a different — and much shorter —
/// command list. See `greet.rs` for why the list is the security boundary.
pub fn run_greeter() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(greet::Session::default());
            // Label "greeter". main.tsx reads it before it renders anything,
            // so the login screen never shows a frame of the desktop first.
            WebviewWindowBuilder::new(app, "greeter", WebviewUrl::App("index.html".into()))
                .title("Khadi")
                .decorations(false)
                .fullscreen(true)
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            theme,
            greet::greet_begin,
            greet::greet_answer,
            greet::greet_start,
            greet::greet_cancel,
            greet::greet_session_cmd,
            greet::greet_host,
        ])
        .run(tauri::generate_context!())
        .expect("khadi-shell --greeter failed to start");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            app.manage(Sources::default());
            app.manage(pty::Ptys::default());
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Khadi")
                .decorations(false)
                .inner_size(1920.0, 1080.0)
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            theme, system, network, filesystem, mounts, workspaces, workspace_goto,
            pty_spawn, pty_write, pty_resize, pty_kill
        ])
        .run(tauri::generate_context!())
        .expect("khadi-shell failed to start");
}
