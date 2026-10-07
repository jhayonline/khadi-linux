//! khadi-hud — the one component Khadi writes from scratch.
//!
//! Phase 0's A/B against eDEX scored the composed chassis at 1 faithful
//! element of 18. The 13 missing ones are this binary's specification: no
//! off-the-shelf TUI draws the bracket-tick header, the block clock, the
//! per-core sparklines or the memory grid, and btop structurally cannot
//! occupy a 34-column side panel at all.
//!
//!   khadi-hud dash        the system column (Phase 3a)
//!   khadi-hud net         the network column
//!   khadi-hud fs          the filesystem strip
//!   khadi-hud header      the two-row label pair above the shell
//!   <cmd> --once          render one frame and exit — for screenshots and CI

mod dash;
mod fs_panel;
mod net_panel;
mod widgets;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use khadi_core::{metrics::Metrics, Theme};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::{Duration, Instant}};

/// Local wall-clock HH:MM.
///
/// Phase 0 locked HH:MM over HH:MM:SS: eight glyphs at a legible width need 39
/// columns and the panel is 34.
fn hhmm() -> String {
    chrono::Local::now().format("%H:%M").to_string()
}

enum Panel {
    Dash,
    Net,
    Fs,
    /// A/B item 10: `TERMINAL / MAIN SHELL`. It is chrome for the shell pane,
    /// and the shell cannot draw it — anything printed into the terminal
    /// scrolls away with the first screenful of output. The Phase 0 gate ruled
    /// out zellij's own frames, so it gets a pane of its own, two rows tall,
    /// which is the same answer the other three panels got.
    Header { left: String, right: String },
}

fn render_once<F>(w: u16, h: u16, draw: F) -> Result<String>
where
    F: FnOnce(&mut ratatui::Frame),
{
    use ratatui::{backend::TestBackend, Terminal as T};
    let mut term = T::new(TestBackend::new(w, h))?;
    term.draw(draw)?;
    let buf = term.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    Ok(out)
}

fn run(panel: Panel) -> Result<()> {
    let theme = Theme::load()?;
    let mut m = Metrics::new();
    let mut n = khadi_core::net::Network::new();
    let mut fsys = khadi_core::disks::Filesystem::new();
    m.refresh();
    n.refresh();
    fsys.refresh();

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    let mut term = Terminal::new(CrosstermBackend::new(out))?;

    let tick = Duration::from_millis(1000);
    let mut last = Instant::now();
    loop {
        term.draw(|f| match &panel {
            Panel::Dash => f.render_widget(
                dash::Dash { m: &m, theme: &theme, clock: hhmm() }, f.area()),
            Panel::Net => f.render_widget(
                net_panel::NetPanel { n: &n, theme: &theme }, f.area()),
            Panel::Fs => f.render_widget(
                fs_panel::FsPanel { fs: &fsys, theme: &theme }, f.area()),
            Panel::Header { left, right } => f.render_widget(
                widgets::Header::new(left, right, &theme), f.area()),
        })?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(k) = event::read()? {
                if matches!(k.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }
        if last.elapsed() >= tick {
            match panel {
                Panel::Dash => m.refresh(),
                Panel::Net => n.refresh(),
                Panel::Fs => fsys.refresh(),
                // Static. ratatui redraws it on resize and that is all it owes.
                Panel::Header { .. } => {}
            }
            last = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("dash");
    // Positional labels, flags excluded, so `header --once` and
    // `header TERMINAL "MAIN SHELL" --once` both mean what they look like.
    let mut labels = args.iter().skip(1).filter(|a| !a.starts_with('-'));
    let panel = match cmd {
        "dash" => Panel::Dash,
        "net" => Panel::Net,
        "fs" => Panel::Fs,
        "header" => Panel::Header {
            left: labels.next().cloned().unwrap_or_else(|| "TERMINAL".into()),
            right: labels.next().cloned().unwrap_or_else(|| "MAIN SHELL".into()),
        },
        other => {
            eprintln!("khadi-hud: unknown command {other:?} (want: dash, net, fs, header)");
            std::process::exit(2);
        }
    };
    if !args.iter().any(|a| a == "--once") {
        return run(panel);
    }
    let theme = Theme::load()?;
    // The side panels are 34 columns by measurement; the header spans the
    // shell column, so it has no business defaulting to a panel's width.
    let (dw, dh) = match panel {
        Panel::Header { .. } => (80, widgets::Header::HEIGHT),
        _ => (34, 44),
    };
    let w: u16 = std::env::var("KHADI_COLS").ok().and_then(|s| s.parse().ok()).unwrap_or(dw);
    let h: u16 = std::env::var("KHADI_ROWS").ok().and_then(|s| s.parse().ok()).unwrap_or(dh);
    let out = match panel {
        Panel::Dash => {
            let mut m = Metrics::new();
            m.refresh();
            std::thread::sleep(Duration::from_millis(250));
            m.refresh();
            render_once(w, h, |f| {
                f.render_widget(dash::Dash { m: &m, theme: &theme, clock: hhmm() }, f.area())
            })?
        }
        Panel::Net => {
            let mut n = khadi_core::net::Network::new();
            n.refresh();
            std::thread::sleep(Duration::from_millis(250));
            n.refresh();
            render_once(w, h, |f| {
                f.render_widget(net_panel::NetPanel { n: &n, theme: &theme }, f.area())
            })?
        }
        Panel::Fs => {
            let mut fsys = khadi_core::disks::Filesystem::new();
            fsys.refresh();
            render_once(w, h, |f| {
                f.render_widget(fs_panel::FsPanel { fs: &fsys, theme: &theme }, f.area())
            })?
        }
        Panel::Header { ref left, ref right } => render_once(w, h, |f| {
            f.render_widget(widgets::Header::new(left, right, &theme), f.area())
        })?,
    };
    print!("{out}");
    Ok(())
}
