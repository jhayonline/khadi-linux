//! A terminal tab: a shell on a PTY, a VT parser, and the egui renderer for its grid.

use crate::theme::Theme;
use eframe::egui::{
    self, Color32, Event, FontId, Key, Modifiers, Pos2, Rect, Response, Stroke, TextFormat, Ui,
    Vec2,
    text::LayoutJob,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// The shape of the terminal's cursor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

/// How the terminal is drawn and behaves: the user's choices, from the settings.
#[derive(Debug, Clone, PartialEq)]
pub struct TermLook {
    pub font_size: f32,
    /// Row height, as a multiple of the font's own.
    pub line_height: f32,
    pub cursor: CursorShape,
    pub cursor_blink: bool,
    /// Show bold text in the bright version of its colour.
    pub bold_bright: bool,
    /// Copy text as soon as it is selected.
    pub copy_on_select: bool,
}

pub struct TermTab {
    parser: vt100::Parser,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    rx: Receiver<Vec<u8>>,
    size: (u16, u16),
    scroll_accum: f32,
    /// Anchor and head of the mouse selection, as (row, col) cells of the visible grid.
    selection: Option<((u16, u16), (u16, u16))>,
    pub exited: bool,
}

impl TermTab {
    /// Spawns `shell` as a login shell in `cwd`. `ctx` is woken whenever output arrives.
    pub fn spawn(shell: &str, cwd: &Path, ctx: egui::Context, scrollback: usize) -> anyhow::Result<TermTab> {
        let (rows, cols) = (24, 80);
        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-l");
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "khadi");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                }
            }
            ctx.request_repaint();
        });

        Ok(TermTab {
            parser: vt100::Parser::new(rows, cols, scrollback),
            writer,
            master: pair.master,
            child,
            rx,
            size: (rows, cols),
            scroll_accum: 0.0,
            selection: None,
            exited: false,
        })
    }

    /// Feeds pending shell output into the parser and notices when the shell has exited.
    pub fn pump(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(bytes) => self.parser.process(&bytes),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.exited = true;
                    break;
                }
            }
        }
        if let Ok(Some(_)) = self.child.try_wait() {
            self.exited = true;
        }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Working directory of the shell, used by the file browser and for new tabs.
    pub fn cwd(&self) -> Option<PathBuf> {
        let pid = self.child.process_id()?;
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    /// Name of the program currently in the foreground of this terminal.
    pub fn foreground_process(&self) -> Option<String> {
        let pid = self.master.process_group_leader()?;
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        Some(comm.trim().to_string())
    }

    /// The selection with its ends in reading order.
    fn selection_range(&self) -> Option<((u16, u16), (u16, u16))> {
        let (a, b) = self.selection?;
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    fn selected_text(&self) -> Option<String> {
        let ((r0, c0), (r1, c1)) = self.selection_range()?;
        let text = self.parser.screen().contents_between(r0, c0, r1, c1 + 1);
        (!text.is_empty()).then_some(text)
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        self.parser.screen_mut().set_size(rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// Translates this frame's input events into bytes for the shell.
    ///
    /// Ctrl+Shift+C copies the selection; plain Ctrl+C always interrupts.
    pub fn handle_events(
        &mut self,
        events: &[Event],
        alt_held: bool,
        shift_held: bool,
        ctx: &egui::Context,
    ) {
        let app_cursor = self.parser.screen().application_cursor();
        let bracketed = self.parser.screen().bracketed_paste();
        let mut out: Vec<u8> = Vec::new();
        for event in events {
            match event {
                Event::Text(text) => {
                    if alt_held {
                        out.push(0x1b);
                    }
                    out.extend_from_slice(text.as_bytes());
                }
                Event::Paste(text) => {
                    if bracketed {
                        out.extend_from_slice(b"\x1b[200~");
                        out.extend_from_slice(text.as_bytes());
                        out.extend_from_slice(b"\x1b[201~");
                    } else {
                        out.extend_from_slice(text.replace('\n', "\r").as_bytes());
                    }
                }
                // egui turns Ctrl+C / Ctrl+X into clipboard commands; the shell wants them.
                Event::Copy => match self.selected_text() {
                    Some(text) if shift_held => ctx.copy_text(text),
                    _ => out.push(0x03),
                },
                Event::Cut => out.push(0x18),
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(bytes) = key_bytes(*key, *modifiers, app_cursor) {
                        out.extend_from_slice(&bytes);
                    }
                }
                _ => {}
            }
        }
        if !out.is_empty() {
            self.selection = None;
            self.parser.screen_mut().set_scrollback(0);
            self.write(&out);
        }
    }

    /// Draws the terminal grid into `rect`, resizing the PTY to fit. `response` is the
    /// interaction result for `rect`, used for scrolling and mouse selection.
    pub fn show(&mut self, ui: &mut Ui, rect: Rect, theme: &Theme, response: &Response, look: &TermLook) {
        let font = FontId::monospace(look.font_size);
        let (cw, natural) = ui.fonts(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let ch = (natural * look.line_height).round().max(1.0);
        // Text sits in the middle of a taller or shorter row.
        let inset = (ch - natural) / 2.0;
        let cols = ((rect.width() / cw).floor() as u16).max(2);
        let rows = ((rect.height() / ch).floor() as u16).max(1);
        self.resize(rows, cols);

        let cell_at = |pos: Pos2| {
            let col = ((pos.x - rect.min.x) / cw).floor().clamp(0.0, (cols - 1) as f32);
            let row = ((pos.y - rect.min.y) / ch).floor().clamp(0.0, (rows - 1) as f32);
            (row as u16, col as u16)
        };
        let pointer = response.interact_pointer_pos();
        if response.drag_started() {
            self.selection = pointer.map(|p| (cell_at(p), cell_at(p)));
        } else if response.dragged() {
            if let (Some(p), Some((anchor, _))) = (pointer, self.selection) {
                self.selection = Some((anchor, cell_at(p)));
            }
        } else if response.clicked() {
            self.selection = None;
        }
        if response.drag_stopped() && look.copy_on_select {
            if let Some(text) = self.selected_text() {
                ui.ctx().copy_text(text);
            }
        }

        if response.hovered() {
            self.scroll_accum += ui.input(|i| i.smooth_scroll_delta.y);
            let lines = (self.scroll_accum / ch).trunc();
            if lines != 0.0 {
                self.scroll_accum -= lines * ch;
                let current = self.parser.screen().scrollback() as f32;
                let target = (current + lines).max(0.0) as usize;
                self.selection = None;
                self.parser.screen_mut().set_scrollback(target);
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, crate::ui::GROUND);
        if let Some(((r0, c0), (r1, c1))) = self.selection_range() {
            for row in r0..=r1.min(rows - 1) {
                let from = if row == r0 { c0 } else { 0 };
                let to = if row == r1 { c1 + 1 } else { cols };
                let min = rect.min + Vec2::new(from as f32 * cw, row as f32 * ch);
                let size = Vec2::new(to.saturating_sub(from) as f32 * cw, ch);
                painter.rect_filled(Rect::from_min_size(min, size), 0.0, theme.selection());
            }
        }

        let screen = self.parser.screen();

        // A blinking cursor is on for half of each second.
        let lit = !look.cursor_blink || ui.input(|i| i.time).rem_euclid(1.0) < 0.5;
        if look.cursor_blink {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        let cursor = (!screen.hide_cursor() && screen.scrollback() == 0 && lit).then(|| {
            let (crow, ccol) = screen.cursor_position();
            let cell = Rect::from_min_size(rect.min + Vec2::new(ccol as f32 * cw, crow as f32 * ch), Vec2::new(cw, ch));
            (crow, ccol, cell)
        });
        if let Some((_, _, cell)) = cursor {
            let shape = match look.cursor {
                CursorShape::Block => cell,
                CursorShape::Bar => Rect::from_min_size(cell.min, Vec2::new(2.0, ch)),
                CursorShape::Underline => Rect::from_min_max(Pos2::new(cell.left(), cell.bottom() - 2.0), cell.max),
            };
            painter.rect_filled(shape, 0.0, theme.cursor);
        }

        for row in 0..rows {
            let mut job = LayoutJob::default();
            job.wrap.max_width = f32::INFINITY;
            let mut run = String::new();
            let mut run_style: Option<CellStyle> = None;
            for col in 0..cols {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let style = CellStyle::of(cell, theme, look.bold_bright);
                if run_style != Some(style) {
                    if let Some(prev) = run_style {
                        job.append(&run, 0.0, prev.format(&font));
                        run.clear();
                    }
                    run_style = Some(style);
                }
                if cell.has_contents() {
                    run.push_str(cell.contents());
                } else {
                    run.push(' ');
                }
            }
            if let Some(prev) = run_style {
                job.append(&run, 0.0, prev.format(&font));
            }
            let galley = ui.fonts(|f| f.layout_job(job));
            let pos = Pos2::new(rect.min.x, rect.min.y + row as f32 * ch + inset);
            painter.galley(pos, galley, theme.term_fg);
        }

        // Under a block cursor the character is drawn again, dark on the cursor's
        // colour, so that it stays readable.
        if let (Some((crow, ccol, cell)), CursorShape::Block) = (cursor, look.cursor) {
            if let Some(under) = screen.cell(crow, ccol).filter(|under| under.has_contents()) {
                painter.text(
                    Pos2::new(cell.left(), cell.top() + inset),
                    egui::Align2::LEFT_TOP,
                    under.contents(),
                    font.clone(),
                    crate::ui::GROUND,
                );
            }
        }
    }
}

impl Drop for TermTab {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

#[derive(Clone, Copy, PartialEq)]
struct CellStyle {
    fg: Color32,
    bg: Color32,
    underline: bool,
    italic: bool,
}

impl CellStyle {
    fn of(cell: &vt100::Cell, theme: &Theme, bold_bright: bool) -> CellStyle {
        let resolve = |color: vt100::Color, default: Color32, bold: bool| match color {
            vt100::Color::Default => default,
            vt100::Color::Idx(i) if bold && bold_bright && i < 8 => theme.indexed(i + 8),
            vt100::Color::Idx(i) => theme.indexed(i),
            vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        };
        let mut fg = resolve(cell.fgcolor(), theme.term_fg, cell.bold());
        let mut bg = resolve(cell.bgcolor(), Color32::TRANSPARENT, false);
        if cell.inverse() {
            let opaque_bg = if bg == Color32::TRANSPARENT {
                crate::ui::GROUND
            } else {
                bg
            };
            bg = fg;
            fg = opaque_bg;
        }
        if cell.dim() {
            fg = fg.gamma_multiply(0.6);
        }
        CellStyle {
            fg,
            bg,
            underline: cell.underline(),
            italic: cell.italic(),
        }
    }

    fn format(self, font: &FontId) -> TextFormat {
        TextFormat {
            font_id: font.clone(),
            color: self.fg,
            background: self.bg,
            italics: self.italic,
            underline: if self.underline {
                Stroke::new(1.0, self.fg)
            } else {
                Stroke::NONE
            },
            ..Default::default()
        }
    }
}

/// Encodes a non-text key press the way xterm does.
fn key_bytes(key: Key, m: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // xterm's modifier parameter: 1, plus 1 for Shift, 2 for Alt, 4 for Ctrl.
    let modifier = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8;
    // Arrows, Home and End: a letter, in one of three forms.
    let cursor = |c: char| {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{c}")
        } else if app_cursor {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
    };
    // Keys named by a number, such as Delete (3) and F5 (15).
    let numbered = |n: u8| {
        if modifier > 1 {
            format!("\x1b[{n};{modifier}~")
        } else {
            format!("\x1b[{n}~")
        }
    };
    let function = |c: char| {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{c}")
        } else {
            format!("\x1bO{c}")
        }
    };
    let s: String = match key {
        // Shift+Enter and Alt+Enter ask for a new line without submitting, in programs
        // that tell the two apart.
        Key::Enter if m.shift || m.alt => "\x1b\r".into(),
        Key::Enter => "\r".into(),
        Key::Tab if m.shift => "\x1b[Z".into(),
        Key::Tab => "\t".into(),
        Key::Backspace if m.alt => "\x1b\x7f".into(),
        Key::Backspace if m.ctrl => "\x17".into(),
        Key::Backspace => "\x7f".into(),
        Key::Escape => "\x1b".into(),
        Key::ArrowUp => cursor('A'),
        Key::ArrowDown => cursor('B'),
        Key::ArrowRight => cursor('C'),
        Key::ArrowLeft => cursor('D'),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Insert => numbered(2),
        Key::Delete => numbered(3),
        Key::PageUp => numbered(5),
        Key::PageDown => numbered(6),
        Key::F1 => function('P'),
        Key::F2 => function('Q'),
        Key::F3 => function('R'),
        Key::F4 => function('S'),
        Key::F5 => numbered(15),
        Key::F6 => numbered(17),
        Key::F7 => numbered(18),
        Key::F8 => numbered(19),
        Key::F9 => numbered(20),
        Key::F10 => numbered(21),
        Key::F11 => numbered(23),
        Key::F12 => numbered(24),
        _ if m.ctrl => {
            let byte = match key {
                Key::Space => 0x00,
                Key::OpenBracket => 0x1b,
                Key::Backslash => 0x1c,
                Key::CloseBracket => 0x1d,
                _ => {
                    let name = key.name();
                    let c = name.bytes().next()?;
                    if name.len() != 1 || !c.is_ascii_alphabetic() {
                        return None;
                    }
                    c.to_ascii_uppercase() & 0x1f
                }
            };
            // Alt on top of Ctrl is an escape prefix, as for plain text.
            return Some(if m.alt { vec![0x1b, byte] } else { vec![byte] });
        }
        _ => return None,
    };
    Some(s.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn encodes_keys() {
        let none = Modifiers::NONE;
        assert_eq!(key_bytes(Key::Enter, none, false).unwrap(), b"\r");
        assert_eq!(key_bytes(Key::ArrowUp, none, false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes(Key::ArrowUp, none, true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes(Key::D, Modifiers::CTRL, false).unwrap(), [0x04]);
        assert_eq!(key_bytes(Key::A, none, false), None);
        assert_eq!(key_bytes(Key::Enter, Modifiers::SHIFT, false).unwrap(), b"\x1b\r");
        assert_eq!(key_bytes(Key::ArrowLeft, Modifiers::ALT, true).unwrap(), b"\x1b[1;3D");
        assert_eq!(key_bytes(Key::ArrowLeft, Modifiers::CTRL, false).unwrap(), b"\x1b[1;5D");
        assert_eq!(key_bytes(Key::ArrowUp, Modifiers::SHIFT, false).unwrap(), b"\x1b[1;2A");
        assert_eq!(key_bytes(Key::Delete, Modifiers::CTRL, false).unwrap(), b"\x1b[3;5~");
        assert_eq!(key_bytes(Key::F5, none, false).unwrap(), b"\x1b[15~");
        assert_eq!(key_bytes(Key::F1, Modifiers::SHIFT, false).unwrap(), b"\x1b[1;2P");
        assert_eq!(key_bytes(Key::B, Modifiers::CTRL | Modifiers::ALT, false).unwrap(), [0x1b, 0x02]);
    }

    #[test]
    fn shell_output_reaches_the_grid() {
        let ctx = egui::Context::default();
        let mut tab = TermTab::spawn("/bin/sh", Path::new("/tmp"), ctx, 100).unwrap();
        assert_eq!(tab.cwd(), Some(PathBuf::from("/tmp")));
        tab.write(b"echo he''llo-pty\r");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            tab.pump();
            if tab.parser.screen().contents().contains("hello-pty") {
                break;
            }
            assert!(Instant::now() < deadline, "no output from shell");
            std::thread::sleep(Duration::from_millis(20));
        }
        let (_, cols) = tab.parser.screen().size();
        // The prompt may or may not precede the output, depending on timing.
        let (row, col) = tab
            .parser
            .screen()
            .rows(0, cols)
            .enumerate()
            .find_map(|(row, line)| Some((row as u16, line.find("hello-pty")? as u16)))
            .unwrap();
        tab.selection = Some(((row, col + 8), (row, col)));
        assert_eq!(tab.selected_text().as_deref(), Some("hello-pty"));

        tab.write(b"exit\r");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !tab.exited {
            tab.pump();
            assert!(Instant::now() < deadline, "shell did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
