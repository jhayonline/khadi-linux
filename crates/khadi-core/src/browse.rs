//! The directory the shell is sitting in, and what is in it.
//!
//! eDEX's FILESYSTEM module is a file BROWSER — an icon grid of the current
//! directory, with a mount-usage bar underneath. Khadi shipped only the bar,
//! on the reasoning that browsing is yazi's job and an icon grid needs a Nerd
//! Font. The second half of that was wrong: the icons are six shapes, and a
//! cell grid can draw shapes. It is the same answer `khadi-hud` already gives
//! for the clock and the memory block.

use std::{fs, path::{Path, PathBuf}, time::SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The `..` entry. eDEX calls it "Go up" and gives it its own icon.
    Up,
    Dir,
    File,
    /// A symlink. eDEX draws a chain and underlines the name; so does Khadi.
    Link,
    /// Sockets, fifos, devices — things that are neither.
    Other,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    /// Dotfiles are listed but dimmed, as eDEX dims them at 0.7 opacity.
    pub hidden: bool,
}

pub struct Browser {
    pub cwd: PathBuf,
    pub entries: Vec<Entry>,
    /// Usage of the filesystem `cwd` lives on, for the bar under the grid.
    pub used: u64,
    pub total: u64,
    pub mount: String,
}

/// The directory the SHELL is in, not the one this panel was launched from.
///
/// eDEX's browser follows its terminal because they are the same process. The
/// panel is a separate one, so it has to go and look: the newest interactive
/// shell this user owns that is attached to a pty. That is the shell you are
/// typing in on a desktop whose whole premise is one terminal, and when it
/// guesses wrong the cost is a panel showing a directory you also own.
fn shell_cwd() -> Option<PathBuf> {
    const SHELLS: [&str; 6] = ["bash", "zsh", "fish", "sh", "nu", "dash"];
    let me = unsafe { libc_getuid() };
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for ent in fs::read_dir("/proc").ok()? {
        let ent = match ent {
            Ok(e) => e,
            Err(_) => continue,
        };
        let p = ent.path();
        if p.file_name().and_then(|s| s.to_str()).is_none_or(|s| !s.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let comm = match fs::read_to_string(p.join("comm")) {
            Ok(c) => c.trim().to_string(),
            Err(_) => continue,
        };
        if !SHELLS.contains(&comm.as_str()) {
            continue;
        }
        // Owned by us, and on a pty rather than a script runner.
        let md = match fs::metadata(&p) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if uid_of(&md) != me {
            continue;
        }
        if !fs::read_link(p.join("fd/0")).map(|t| t.to_string_lossy().starts_with("/dev/pts/"))
            .unwrap_or(false)
        {
            continue;
        }
        let started = md.modified().ok()?;
        let cwd = match fs::read_link(p.join("cwd")) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if best.as_ref().is_none_or(|(t, _)| started > *t) {
            best = Some((started, cwd));
        }
    }
    best.map(|(_, c)| c)
}

#[cfg(target_os = "linux")]
fn uid_of(md: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    md.uid()
}

unsafe fn libc_getuid() -> u32 {
    // One libc call, not worth a dependency. `getuid` cannot fail and has no
    // errno, which is why it is safe to call like this.
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

impl Browser {
    pub fn new() -> Self {
        Self {
            cwd: PathBuf::new(),
            entries: Vec::new(),
            used: 0,
            total: 0,
            mount: String::new(),
        }
    }

    pub fn refresh(&mut self) {
        let cwd = shell_cwd()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));
        self.cwd = cwd.clone();
        self.entries = Self::list(&cwd);
        let (used, total, mount) = statvfs(&cwd);
        self.used = used;
        self.total = total;
        self.mount = mount;
    }

    fn list(dir: &Path) -> Vec<Entry> {
        let mut out = Vec::new();
        if dir.parent().is_some() {
            out.push(Entry { name: "UP".into(), kind: Kind::Up, size: 0, hidden: false });
        }
        let rd = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => return out,
        };
        let mut v: Vec<Entry> = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // symlink_metadata, not metadata: a link to a directory is a LINK,
            // which is the distinction the icon is there to make.
            let md = match e.path().symlink_metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let ft = md.file_type();
            let kind = if ft.is_symlink() {
                Kind::Link
            } else if ft.is_dir() {
                Kind::Dir
            } else if ft.is_file() {
                Kind::File
            } else {
                Kind::Other
            };
            let hidden = name.starts_with('.');
            v.push(Entry { name, kind, size: md.len(), hidden });
        }
        // Directories first, then everything else, each alphabetically and
        // case-insensitively — the order a person expects a browser to use.
        v.sort_by(|a, b| {
            let rank = |k: Kind| match k {
                Kind::Up => 0,
                Kind::Dir => 1,
                Kind::Link => 2,
                _ => 3,
            };
            rank(a.kind)
                .cmp(&rank(b.kind))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out.extend(v);
        out
    }
}

impl Default for Browser {
    fn default() -> Self {
        Self::new()
    }
}

/// Usage of whichever filesystem a path lives on.
///
/// `sysinfo` enumerates mounts but will not answer "which one is this path
/// on", and the browser's bar is about the directory on screen rather than
/// about `/`.
fn statvfs(path: &Path) -> (u64, u64, String) {
    #[repr(C)]
    #[derive(Default)]
    struct Statvfs {
        bsize: u64,
        frsize: u64,
        blocks: u64,
        bfree: u64,
        bavail: u64,
        files: u64,
        ffree: u64,
        favail: u64,
        fsid: u64,
        flag: u64,
        namemax: u64,
        spare: [u32; 6],
    }
    unsafe extern "C" {
        fn statvfs(path: *const u8, buf: *mut Statvfs) -> i32;
    }
    let mut c: Vec<u8> = path.to_string_lossy().as_bytes().to_vec();
    c.push(0);
    let mut s = Statvfs::default();
    let ok = unsafe { statvfs(c.as_ptr(), &mut s) } == 0;
    if !ok || s.blocks == 0 {
        return (0, 0, String::new());
    }
    let unit = if s.frsize > 0 { s.frsize } else { s.bsize };
    let total = s.blocks * unit;
    let used = (s.blocks - s.bfree) * unit;
    (used, total, mount_of(path))
}

/// The mount point a path sits under, longest match from /proc/mounts.
fn mount_of(path: &Path) -> String {
    let txt = match fs::read_to_string("/proc/self/mounts") {
        Ok(t) => t,
        Err(_) => return "/".into(),
    };
    let p = path.to_string_lossy();
    let mut best = String::from("/");
    for line in txt.lines() {
        let mut f = line.split_whitespace();
        let (_dev, mnt) = (f.next(), f.next());
        let mnt = match mnt {
            Some(m) => m,
            None => continue,
        };
        if p.starts_with(mnt) && mnt.len() > best.len() {
            best = mnt.to_string();
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_a_real_directory_dirs_first() {
        let mut b = Browser::new();
        b.refresh();
        assert!(!b.entries.is_empty(), "no entries for {:?}", b.cwd);
        assert_eq!(b.entries[0].kind, Kind::Up, "the up entry comes first");
        // Once a non-directory appears, no directory may follow it.
        let mut seen_file = false;
        for e in &b.entries {
            match e.kind {
                Kind::Up | Kind::Dir => assert!(!seen_file, "{} came after a file", e.name),
                _ => seen_file = true,
            }
        }
    }

    #[test]
    fn the_usage_bar_has_a_denominator() {
        let mut b = Browser::new();
        b.refresh();
        assert!(b.total > 0, "statvfs gave no total for {:?}", b.cwd);
        assert!(b.used <= b.total);
        assert!(b.mount.starts_with('/'), "mount point is {:?}", b.mount);
    }

    /// `/` has no parent, so it must not offer to go up to itself.
    #[test]
    fn root_has_no_up_entry() {
        let v = Browser::list(Path::new("/"));
        assert!(v.iter().all(|e| e.kind != Kind::Up));
    }
}
