//! Serves the desktop protocol (see `khadi_common::ipc`) to the shell.

use khadi_common::ipc::{Command, DesktopState};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

struct Client {
    stream: UnixStream,
    pending: Vec<u8>,
}

pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
    clients: Vec<Client>,
    /// The most recent state line, sent to clients as they connect.
    last: String,
    /// Likewise for the displays line.
    last_displays: String,
}

impl IpcServer {
    pub fn bind(path: PathBuf) -> std::io::Result<IpcServer> {
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        Ok(IpcServer {
            listener,
            path,
            clients: Vec::new(),
            last: DesktopState::default().encode(),
            last_displays: String::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Accepts new clients and returns the commands received since the last call.
    /// Never blocks; meant to be called once per frame.
    pub fn poll(&mut self) -> Vec<Command> {
        while let Ok((mut stream, _)) = self.listener.accept() {
            if stream.set_nonblocking(true).is_ok()
                && stream.write_all(self.last.as_bytes()).is_ok()
                && stream.write_all(self.last_displays.as_bytes()).is_ok()
            {
                self.clients.push(Client {
                    stream,
                    pending: Vec::new(),
                });
            }
        }

        let mut commands = Vec::new();
        self.clients.retain_mut(|client| {
            let mut buf = [0u8; 512];
            loop {
                match client.stream.read(&mut buf) {
                    Ok(0) => return false,
                    Ok(n) => client.pending.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => return false,
                }
            }
            while let Some(end) = client.pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = client.pending.drain(..=end).collect();
                if let Some(command) = Command::decode(String::from_utf8_lossy(&line).trim_end()) {
                    commands.push(command);
                }
            }
            // A client that never finishes a line is not speaking the protocol.
            client.pending.len() < 4096
        });
        commands
    }

    /// Sends one protocol line to every client.
    pub fn send(&mut self, line: &str) {
        self.clients
            .retain_mut(|client| client.stream.write_all(line.as_bytes()).is_ok());
    }

    /// Sends `state` to every client, unless it is what they already have.
    pub fn publish(&mut self, state: &DesktopState) {
        let line = state.encode();
        if line == self.last {
            return;
        }
        self.send(&line);
        self.last = line;
    }
}

impl IpcServer {
    /// Sends the monitors to every client, unless nothing about them changed.
    pub fn publish_displays(&mut self, displays: &khadi_common::ipc::Displays) {
        let line = displays.encode();
        if line != self.last_displays {
            self.send(&line);
            self.last_displays = line;
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    #[test]
    fn serves_state_and_receives_commands() {
        let path = std::env::temp_dir().join(format!("khadi-ipc-test-{}.sock", std::process::id()));
        let mut server = IpcServer::bind(path.clone()).unwrap();

        let mut client = UnixStream::connect(&path).unwrap();
        let mut reader = BufReader::new(client.try_clone().unwrap());
        assert!(server.poll().is_empty());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(DesktopState::decode(&line), Some(DesktopState::default()));

        let state = DesktopState {
            screens: 2,
            focus: Some(0),
            apps: vec![khadi_common::ipc::AppInfo {
                title: "Editor".into(),
                screen: 1,
                place: khadi_common::ipc::Place::Main,
            }],
            ..Default::default()
        };
        server.publish(&state);
        line.clear();
        reader.read_line(&mut line).unwrap();
        assert_eq!(DesktopState::decode(&line), Some(state));

        // A split write and a junk line must still yield exactly the valid commands.
        client.write_all(b"activate 0\nnonsense\nclo").unwrap();
        client.write_all(b"se 0\n").unwrap();
        let mut commands = Vec::new();
        for _ in 0..50 {
            commands.extend(server.poll());
            if commands.len() == 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(commands, [Command::Activate(0), Command::Close(0)]);

        drop(server);
        assert!(!path.exists());
    }
}
