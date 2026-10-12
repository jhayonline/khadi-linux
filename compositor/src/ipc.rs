//! Serves the desktop protocol (see `des_common::ipc`) to the shell.

use des_common::ipc::{Command, DesktopState};
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
    /// Likewise for whether the screen is locked.
    last_locked: String,
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
            last_locked: des_common::ipc::Event::locked(false),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Accepts new clients and returns the commands received since the last call.
    /// Never blocks; meant to be called once per frame.
    pub fn poll(&mut self) -> Vec<Command> {
        while let Ok((mut stream, _)) = self.listener.accept() {
            if stream.set_nonblocking(true).is_ok() {
                // A client that has already left cannot be greeted, but may have said
                // something first, which the reading below still takes in.
                let _ = stream
                    .write_all(self.last.as_bytes())
                    .and_then(|()| stream.write_all(self.last_displays.as_bytes()))
                    .and_then(|()| stream.write_all(self.last_locked.as_bytes()));
                self.clients.push(Client {
                    stream,
                    pending: Vec::new(),
                });
            }
        }

        let mut commands = Vec::new();
        self.clients.retain_mut(|client| {
            let mut buf = [0u8; 512];
            // A client may say its piece and leave at once: what it said still counts.
            let mut gone = false;
            loop {
                match client.stream.read(&mut buf) {
                    Ok(0) => {
                        gone = true;
                        break;
                    }
                    Ok(n) => client.pending.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => {
                        gone = true;
                        break;
                    }
                }
            }
            while let Some(end) = client.pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = client.pending.drain(..=end).collect();
                if let Some(command) = Command::decode(String::from_utf8_lossy(&line).trim_end()) {
                    commands.push(command);
                }
            }
            // A client that never finishes a line is not speaking the protocol.
            !gone && client.pending.len() < 4096
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
    /// Tells every client whether the screen is locked, when that changes. A shell
    /// started while it is locked is told as it connects.
    pub fn publish_locked(&mut self, locked: bool) {
        let line = des_common::ipc::Event::locked(locked);
        if line != self.last_locked {
            self.send(&line);
            self.last_locked = line;
        }
    }

    /// Sends the monitors to every client, unless nothing about them changed.
    pub fn publish_displays(&mut self, displays: &des_common::ipc::Displays) {
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
        let path = std::env::temp_dir().join(format!("des-ui-ipc-test-{}.sock", std::process::id()));
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
            apps: vec![des_common::ipc::AppInfo {
                title: "Editor".into(),
                screen: 1,
                place: des_common::ipc::Place::Main,
            }],
            ..Default::default()
        };
        server.publish(&state);
        // Other things a new client is told, such as the lock, come in between.
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line.starts_with("state ") {
                break;
            }
            assert_eq!(line, "locked 0\n");
        }
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
