//! Mount points and their usage.
//!
//! eDEX's FILESYSTEM panel is two things wearing one name: a clickable icon
//! grid, and a "Mount /home/x used 71%" bar. Khadi splits them. Browsing is
//! yazi's job — it is keyboard-driven, already themed, and an icon grid needs
//! a Nerd Font that Khadi deliberately does not ship (Phase 1 measured the
//! tofu). What has no home anywhere else is the mount usage, so that is what
//! lives here.

use sysinfo::Disks;

#[derive(Debug, Clone)]
pub struct Mount {
    pub name: String,
    pub path: String,
    pub total: u64,
    pub used: u64,
    pub removable: bool,
}

impl Mount {
    pub fn frac(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f64 / self.total as f64).clamp(0.0, 1.0)
        }
    }
}

pub struct Filesystem {
    disks: Disks,
    pub mounts: Vec<Mount>,
}

impl Filesystem {
    pub fn new() -> Self {
        Self { disks: Disks::new_with_refreshed_list(), mounts: Vec::new() }
    }

    pub fn refresh(&mut self) {
        self.disks.refresh(true);
        let mut v: Vec<Mount> = self
            .disks
            .list()
            .iter()
            .filter(|d| d.total_space() > 0)
            .map(|d| Mount {
                name: d.name().to_string_lossy().to_string(),
                path: d.mount_point().to_string_lossy().to_string(),
                total: d.total_space(),
                used: d.total_space().saturating_sub(d.available_space()),
                removable: d.is_removable(),
            })
            .collect();

        // A btrfs root reports every subvolume as a separate mount with
        // identical totals, which fills the panel with the same number six
        // times. Keep the shortest path per (name, total) pair — that is the
        // one a person recognises.
        v.sort_by(|a, b| a.path.len().cmp(&b.path.len()));
        let mut seen = Vec::new();
        v.retain(|m| {
            let key = (m.name.clone(), m.total);
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        });
        v.sort_by(|a, b| a.path.cmp(&b.path));
        self.mounts = v;
    }
}

impl Default for Filesystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frac_is_bounded_and_safe() {
        let m = Mount { name: "x".into(), path: "/".into(), total: 0, used: 10, removable: false };
        assert_eq!(m.frac(), 0.0, "a zero-size mount must not divide by zero");
        let m = Mount { name: "x".into(), path: "/".into(), total: 100, used: 250, removable: false };
        assert_eq!(m.frac(), 1.0, "over-full must clamp, not overflow the bar");
    }

    #[test]
    fn subvolumes_are_collapsed() {
        let mut fsys = Filesystem::new();
        fsys.refresh();
        let mut seen: Vec<(String, u64)> = Vec::new();
        for m in &fsys.mounts {
            let k = (m.name.clone(), m.total);
            assert!(!seen.contains(&k), "duplicate mount {k:?} reached the panel");
            seen.push(k);
        }
    }
}
