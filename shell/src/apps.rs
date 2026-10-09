//! Installed applications: reads the system's desktop entries and starts programs.

use std::collections::HashSet;
use std::os::unix::process::CommandExt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// One launchable application, from a `.desktop` file.
#[derive(Debug, Clone, PartialEq)]
pub struct AppEntry {
    pub name: String,
    /// The program and its arguments.
    pub exec: Vec<String>,
    /// Whether it must run inside a terminal.
    pub terminal: bool,
}

/// The directories that hold desktop entries, most specific first.
fn entry_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());

    let mut dirs = vec![data_home.join("applications")];
    dirs.extend(data_dirs.split(':').map(|dir| PathBuf::from(dir).join("applications")));
    // Snap and Flatpak add themselves to XDG_DATA_DIRS only in some sessions.
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs.push(data_home.join("flatpak/exports/share/applications"));
    dirs
}

/// Every application the user could start, sorted by name.
pub fn scan() -> Vec<AppEntry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for dir in entry_dirs() {
        let Ok(listing) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in listing.flatten() {
            let name = file.file_name();
            if !name.to_string_lossy().ends_with(".desktop") {
                continue;
            }
            // An entry in an earlier directory overrides the same file in a later one,
            // even if it only exists to hide the application.
            if !seen.insert(name) {
                continue;
            }
            if let Some(entry) = std::fs::read_to_string(file.path()).ok().and_then(|text| parse(&text)) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries.dedup_by(|a, b| a.name == b.name && a.exec == b.exec);
    entries
}

/// Reads one desktop entry. Returns `None` for anything that should not be listed.
pub fn parse(text: &str) -> Option<AppEntry> {
    let (mut name, mut exec, mut terminal) = (None, None, false);
    let mut in_main_group = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main_group = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !in_main_group {
            continue;
        }
        let value = value.trim();
        match key.trim() {
            "Type" if value != "Application" => return None,
            "NoDisplay" | "Hidden" if value == "true" => return None,
            // Entries meant for one desktop only are that desktop's own settings pages.
            "OnlyShowIn" => return None,
            "Name" => name = Some(value.to_string()),
            "Exec" => exec = Some(split_exec(value)),
            "Terminal" => terminal = value == "true",
            _ => {}
        }
    }
    let exec = exec.filter(|exec| !exec.is_empty())?;
    Some(AppEntry {
        name: name?,
        exec,
        terminal,
    })
}

/// Splits an `Exec` value into arguments, following the desktop entry rules: double
/// quotes group words, a backslash escapes inside them, and `%f`-style placeholders
/// (which stand for files to open) are dropped.
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut has_current = false;
    let mut quoted = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has_current = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '%' => match chars.next() {
                Some('%') => {
                    current.push('%');
                    has_current = true;
                }
                // A placeholder, or a stray percent sign at the end.
                _ => {}
            },
            c if c.is_whitespace() && !quoted => {
                if has_current {
                    args.push(std::mem::take(&mut current));
                    has_current = false;
                }
            }
            c => {
                current.push(c);
                has_current = true;
            }
        }
    }
    if has_current {
        args.push(current);
    }
    // Dropping a placeholder can leave an empty word behind, as in `--name=%c`.
    args.retain(|arg| !arg.is_empty());
    args
}

/// Starts an application on its own, detached from the shell.
pub fn launch(entry: &AppEntry) -> std::io::Result<()> {
    let (program, args) = entry
        .exec
        .split_first()
        .ok_or_else(|| std::io::Error::other("nothing to run"))?;
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null());
    // What the application prints is kept, to explain a window that never appears.
    match output_log(&entry.exec.join(" ")) {
        Some((out, err)) => command.stdout(out).stderr(err),
        None => command.stdout(Stdio::null()).stderr(Stdio::null()),
    };
    command
        // Its own process group: closing a terminal tab must not take it down.
        .process_group(0);
    if let Some(home) = std::env::var_os("HOME") {
        command.current_dir(home);
    }
    let mut child = command.spawn()?;
    // Collect the exit status when it comes, so no zombie process is left behind.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Opens `apps.log` in the state directory for a new application's output, starting
/// the file over once it has grown large.
fn output_log(command_line: &str) -> Option<(Stdio, Stdio)> {
    let state_home = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    let dir = state_home.join("edex-rs");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("apps.log");
    let large = std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 1024 * 1024);
    let mut file = OpenOptions::new()
        .create(true)
        .append(!large)
        .write(true)
        .truncate(large)
        .open(path)
        .ok()?;
    let _ = writeln!(file, "\n=== {} {command_line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    Some((Stdio::from(file.try_clone().ok()?), Stdio::from(file)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_exec_lines() {
        assert_eq!(split_exec("firefox %u"), ["firefox"]);
        assert_eq!(
            split_exec("/usr/bin/brave-browser-stable --profile-directory=Default %U"),
            ["/usr/bin/brave-browser-stable", "--profile-directory=Default"]
        );
        assert_eq!(
            split_exec(r#"env FOO=1 "/opt/My App/run" --title "a \"b\"" 100%%"#),
            ["env", "FOO=1", "/opt/My App/run", "--title", r#"a "b""#, "100%"]
        );
        assert_eq!(split_exec("app --name=%c --flag"), ["app", "--name=", "--flag"]);
        assert_eq!(split_exec("  "), Vec::<String>::new());
    }

    #[test]
    fn parses_an_application() {
        let entry = parse(
            "[Desktop Entry]\nType=Application\nName=Text Editor\nName[de]=Texteditor\n\
             Exec=gnome-text-editor %U\nTerminal=false\n\n[Desktop Action new-window]\n\
             Name=New Window\nExec=gnome-text-editor --new-window\n",
        );
        assert_eq!(
            entry,
            Some(AppEntry {
                name: "Text Editor".into(),
                exec: vec!["gnome-text-editor".into()],
                terminal: false,
            })
        );
    }

    #[test]
    fn skips_what_should_not_be_listed() {
        assert_eq!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\nNoDisplay=true\n"), None);
        assert_eq!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\nHidden=true\n"), None);
        assert_eq!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\nOnlyShowIn=GNOME;\n"), None);
        assert_eq!(parse("[Desktop Entry]\nType=Link\nName=A\nURL=http://example.org\n"), None);
        assert_eq!(parse("[Desktop Entry]\nType=Application\nName=A\n"), None);
        assert_eq!(parse("not a desktop file"), None);
    }

    #[test]
    fn launches_a_program() {
        let marker = std::env::temp_dir().join(format!("edex-launch-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let entry = AppEntry {
            name: "touch".into(),
            exec: vec!["touch".into(), marker.to_string_lossy().into_owned()],
            terminal: false,
        };
        launch(&entry).unwrap();
        let appeared = (0..100).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            marker.exists()
        });
        let _ = std::fs::remove_file(&marker);
        assert!(appeared);
        assert!(launch(&AppEntry { exec: vec!["/nonexistent/program".into()], ..entry }).is_err());
    }

    #[test]
    fn reads_terminal_flag() {
        let entry = parse("[Desktop Entry]\nType=Application\nName=htop\nExec=htop\nTerminal=true\n").unwrap();
        assert!(entry.terminal);
    }
}
