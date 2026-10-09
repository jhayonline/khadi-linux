//! The shell's link to the compositor: which applications are open, and requests to
//! switch between them. Absent when the shell runs on its own.

use khadi_common::ipc::{Command, DesktopState, Displays, Event, SOCKET_ENV};
use eframe::egui;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{Receiver, channel};

pub struct Desktop {
    writer: UnixStream,
    rx: Receiver<Event>,
    pub state: DesktopState,
    /// Set when the compositor's launcher shortcut was pressed; cleared by the reader.
    pub launcher_requested: bool,
    /// Likewise for the settings shortcut.
    pub settings_requested: bool,
    /// The monitors, as last reported.
    pub displays: Displays,
    heard: bool,
}

impl Desktop {
    /// Connects to the compositor named by the environment, if there is one.
    pub fn connect(ctx: egui::Context) -> Option<Desktop> {
        let path = std::env::var_os(SOCKET_ENV)?;
        let writer = match UnixStream::connect(&path) {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("khadi: cannot reach compositor at {}: {e}", path.display());
                return None;
            }
        };
        let reader = BufReader::new(writer.try_clone().ok()?);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Some(event) = Event::decode(&line) {
                    if tx.send(event).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            }
        });
        Some(Desktop {
            writer,
            rx,
            state: DesktopState::default(),
            launcher_requested: false,
            settings_requested: false,
            displays: Displays::default(),
            heard: false,
        })
    }

    /// Takes in state updates that arrived since the last frame.
    pub fn pump(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::State(state) => {
                    self.state = state;
                    self.heard = true;
                }
                Event::Launcher => self.launcher_requested = true,
                Event::Settings => self.settings_requested = true,
                Event::Displays(displays) => self.displays = displays,
            }
        }
    }

    /// Whether the compositor has reported its state yet.
    pub fn connected(&self) -> bool {
        self.heard
    }

    pub fn send(&mut self, command: Command) {
        let _ = self.writer.write_all(command.encode().as_bytes());
    }
}
