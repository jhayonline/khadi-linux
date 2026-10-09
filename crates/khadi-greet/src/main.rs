//! khadi-greet — the login screen.
//!
//! The first thing seen after the splash, and the thing seen most often after
//! that. tuigreet themes its colours and draws a box; it cannot draw the
//! bracket-tick header or the block clock, so the login screen was the one
//! surface still wearing someone else's design.
//!
//! The widgets are in `chrome`, carried over when khadi-hud was retired.
//! system panel are the same code.
//!
//! **greetd owns authentication.** This process collects a string, hands it
//! over and is told yes or no; PAM is never touched here. It also never logs
//! the string, and clears the buffer as soon as it is sent.

mod chrome;

use anyhow::Result;
use chrono::Local;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use khadi_core::greetd::{Greetd, Request, Response};
use khadi_core::Theme;
use chrome::{col, BigClock, Header};
use ratatui::{
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::Widget,
    Terminal,
};
use std::{io, time::Duration};

/// What the greeter is waiting for. greetd-ipc(7) is explicit that there is no
/// limit on the number or kind of auth messages and that a greeter must not
/// assume — so this is driven by what arrives, not by a fixed
/// username-then-password script.
enum Stage {
    Username,
    Auth { prompt: String, secret: bool },
    Done,
}

struct App {
    stage: Stage,
    username: String,
    input: String,
    message: String,
    cmd: Vec<String>,
}

impl App {
    fn field(&self) -> (&str, String) {
        match &self.stage {
            Stage::Username => ("LOGIN", self.username.clone()),
            // A secret is shown as its length and nothing else.
            Stage::Auth { prompt, secret } => (
                prompt.trim_end_matches([':', ' ']),
                if *secret { "█".repeat(self.input.chars().count()) } else { self.input.clone() },
            ),
            Stage::Done => ("STARTING", String::new()),
        }
    }

    fn buffer(&mut self) -> &mut String {
        match self.stage {
            Stage::Username => &mut self.username,
            _ => &mut self.input,
        }
    }
}

fn draw(f: &mut ratatui::Frame, app: &App, theme: &Theme) {
    let area = f.area();
    Panel { app, theme }.render(area, f.buffer_mut());
}

struct Panel<'a> {
    app: &'a App,
    theme: &'a Theme,
}

impl Widget for Panel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let t = self.theme;
        // A fixed block, centred. eDEX's chrome is a column of label pairs on
        // a hairline; a full-width login form would read as a terminal, not as
        // the product.
        let w = 54u16.min(area.width.saturating_sub(2));
        let h = 14u16;
        if area.width < 20 || area.height < h {
            return;
        }
        let x = area.x + (area.width - w) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;

        Header::new("KHADI", "LOGIN", t).render(Rect::new(x, y, w, 2), buf);

        // The clock, as prominent here as it is on the system panel — eDEX's
        // single most dominant element.
        // Seconds, and the same seven-segment face the system panel uses. The
        // loop already wakes every 500ms to poll for keys, so the hand moves.
        let clock = Local::now().format("%H:%M:%S").to_string();
        // The widget centres itself in whatever it is given, so the box width
        // is the honest argument — the old `len * 6` was the width of a face
        // this no longer draws.
        BigClock { text: &clock, theme: t }
            .render(Rect::new(x, y + 3, w, BigClock::HEIGHT), buf);

        let date = Local::now().format("%a %d %b %Y").to_string().to_uppercase();
        let dx = x + (w.saturating_sub(date.chars().count() as u16)) / 2;
        buf.set_string(dx, y + 9, &date, Style::default().fg(col(t.text_muted)));

        // The field. Label left, value right — the same pair the panels use.
        let (label, value) = self.app.field();
        let ly = y + 11;
        buf.set_string(x, ly, label, Style::default().fg(col(t.text_dim)));
        let vx = x + label.chars().count() as u16 + 2;
        let room = w.saturating_sub(label.chars().count() as u16 + 3) as usize;
        let shown: String = value.chars().rev().take(room).collect::<Vec<_>>()
            .into_iter().rev().collect();
        buf.set_string(vx, ly, &shown, Style::default().fg(col(t.text)));
        buf.set_string(vx + shown.chars().count() as u16, ly, "█",
                       Style::default().fg(col(t.text)));

        if !self.app.message.is_empty() {
            let m: String = self.app.message.chars().take(w as usize).collect();
            buf.set_string(x, y + 13, &m, Style::default().fg(col(t.text_muted)));
        }
    }
}

/// Drive the protocol until it wants something from the user again.
fn advance(g: &mut Greetd, app: &mut App, mut resp: Response) -> Result<()> {
    loop {
        match resp {
            Response::Success => match app.stage {
                // Authentication is done: ask for the session and leave. greetd
                // starts it once this process exits.
                Stage::Done => return Ok(()),
                _ => {
                    app.stage = Stage::Done;
                    g.request(&Request::StartSession {
                        cmd: app.cmd.clone(),
                        env: vec![],
                    })?;
                    return Ok(());
                }
            },
            Response::AuthMessage { auth_message_type, auth_message } => {
                match auth_message_type.as_str() {
                    "secret" | "visible" => {
                        app.input.clear();
                        app.stage = Stage::Auth {
                            prompt: auth_message,
                            secret: auth_message_type == "secret",
                        };
                        return Ok(());
                    }
                    // info and error carry no question, so they are answered
                    // immediately with no response and shown to the user.
                    _ => {
                        app.message = auth_message;
                        resp = g.request(&Request::PostAuthMessageResponse { response: None })?;
                    }
                }
            }
            Response::Error { error_type, description } => {
                app.message = if error_type == "auth_error" {
                    "authentication failed".into()
                } else {
                    description
                };
                // The session under configuration has to be cancelled before
                // another can be created, or the next attempt errors too.
                let _ = g.request(&Request::CancelSession);
                app.input.clear();
                app.stage = Stage::Username;
                return Ok(());
            }
        }
    }
}

/// Repaint the Linux console's 16-colour palette with Khadi's ramp.
///
/// The greeter runs on a bare VT, which has sixteen colours and no more. Khadi
/// draws in truecolor like every other surface, and the console quantises that
/// to its nearest palette entry — so the first working version came out plain
/// white on black, the whole flattened ramp collapsed to one step.
///
/// The palette is not fixed, though: OSC `P<n><rrggbb>` redefines an entry.
/// Setting ALL SIXTEEN to steps of the theme's own ramp means whatever the
/// console picks is on-palette, without the widgets needing to know they are
/// on a VT. `\e]R` puts it back.
///
/// Only on TERM=linux. Under a real terminal emulator the truecolor is used
/// directly and repainting the palette would be vandalism.
fn vt_palette(theme: &Theme) -> bool {
    if std::env::var("TERM").as_deref() != Ok("linux") {
        return false;
    }
    let r = &theme.ramp; // a10..a90
    let entries: [(usize, khadi_core::Rgb); 16] = [
        (0, theme.ground), (1, r[1]), (2, r[2]), (3, r[3]),
        (4, r[4]), (5, r[5]), (6, theme.text_dim), (7, theme.text_muted),
        (8, r[0]), (9, r[3]), (10, r[4]), (11, r[5]),
        (12, r[6]), (13, r[7]), (14, theme.text_dim), (15, theme.text),
    ];
    let mut out = String::new();
    for (i, c) in entries {
        out.push_str(&format!("\x1b]P{:X}{:02x}{:02x}{:02x}", i, c.r, c.g, c.b));
    }
    print!("{out}");
    use std::io::Write as _;
    let _ = io::stdout().flush();
    true
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // What to run once authentication succeeds. greetd's config passes this,
    // the same way it passed --cmd to tuigreet.
    let cmd: Vec<String> = match args.iter().position(|a| a == "--cmd") {
        Some(i) => args[i + 1..].to_vec(),
        None => vec!["start-hyprland".into()],
    };

    let theme = Theme::load()?;
    let repainted = vt_palette(&theme);
    let mut g = Greetd::connect()?;
    let mut app = App {
        stage: Stage::Username,
        username: String::new(),
        input: String::new(),
        message: String::new(),
        cmd,
    };

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    let mut term = Terminal::new(CrosstermBackend::new(out))?;

    let res = run(&mut term, &mut g, &mut app, &theme);

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    if repainted {
        // Hand the console back as it was found. The session that follows is
        // not Khadi's to restyle from here.
        print!("\x1b]R");
        use std::io::Write as _;
        let _ = io::stdout().flush();
    }
    res
}

fn run(
    term: &mut Terminal<CrosstermBackend<io::Stdout>>,
    g: &mut Greetd,
    app: &mut App,
    theme: &Theme,
) -> Result<()> {
    loop {
        term.draw(|f| draw(f, app, theme))?;
        if matches!(app.stage, Stage::Done) {
            return Ok(());
        }
        // The clock has to keep moving while nobody is typing.
        if !event::poll(Duration::from_millis(500))? {
            continue;
        }
        let Event::Key(k) = event::read()? else { continue };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        match k.code {
            KeyCode::Enter => {
                app.message.clear();
                let resp = match &app.stage {
                    Stage::Username => {
                        if app.username.is_empty() {
                            continue;
                        }
                        g.request(&Request::CreateSession { username: app.username.clone() })?
                    }
                    Stage::Auth { .. } => {
                        let secret = std::mem::take(&mut app.input);
                        let r = g.request(&Request::PostAuthMessageResponse {
                            response: Some(secret.clone()),
                        })?;
                        // Not a guarantee against a heap scrape, but the
                        // buffer does not linger for the life of the process.
                        drop(secret);
                        r
                    }
                    Stage::Done => continue,
                };
                advance(g, app, resp)?;
            }
            KeyCode::Backspace => {
                app.buffer().pop();
            }
            KeyCode::Esc => {
                let _ = g.request(&Request::CancelSession);
                app.input.clear();
                app.username.clear();
                app.message.clear();
                app.stage = Stage::Username;
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                app.buffer().clear();
            }
            KeyCode::Char(c) => {
                if !k.modifiers.contains(KeyModifiers::CONTROL) {
                    app.buffer().push(c);
                }
            }
            _ => {}
        }
    }
}
