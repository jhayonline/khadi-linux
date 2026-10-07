//! The angled tab strip, bound to Hyprland's workspaces.
//!
//! eDEX's tabs are shells: tab 0 is `MAIN SHELL`, the other four read `EMPTY`
//! until something runs in them. Khadi's equivalent of a shell is a workspace,
//! and `hyprland.conf` binds exactly five of them to `Super+1..5`, so the
//! mapping is one tab per bound workspace and the count matches the reference.
//!
//! The widgets are built once and only their text and active class change.
//! Rebuilding the strip on every workspace switch would re-run GTK's layout on
//! a skewed container several times a second for no visible gain.

use gtk4::{prelude::*, Label, Orientation};
use khadi_core::hypr::{Snapshot, Workspace};

/// Workspaces reachable from the keyboard — `Super+1..5` in `hyprland.conf`.
/// Changing one without the other leaves a tab nothing can focus.
pub const SLOTS: i32 = 5;

/// What a tab says about a workspace.
///
/// A workspace the user has named keeps its name: that is information the
/// numeric fallback does not have. Hyprland names unnamed workspaces after
/// their own id, and a bare "3" is not a label, so those get eDEX's
/// vocabulary instead.
fn tab_label(ws: Option<&Workspace>) -> String {
    match ws {
        Some(w) if w.windows > 0 => {
            if w.name == w.id.to_string() {
                if w.id == 1 {
                    "MAIN SHELL".to_string()
                } else {
                    format!("SHELL {}", w.id)
                }
            } else {
                // `special:magic` is a Hyprland prefix, not part of the name.
                w.name
                    .strip_prefix("special:")
                    .unwrap_or(&w.name)
                    .to_uppercase()
            }
        }
        _ => "EMPTY".to_string(),
    }
}

/// One tab: a skewed box with an un-skewed label inside it, which is precisely
/// how eDEX does it — `skewX(35deg)` on the container, `skewX(-35deg)` on the
/// text so it stays upright.
struct Tab {
    root: gtk4::Box,
    label: Label,
}

impl Tab {
    fn new() -> Self {
        let root = gtk4::Box::new(Orientation::Horizontal, 0);
        root.add_css_class("tab");
        let label = Label::new(None);
        label.add_css_class("tab-label");
        root.append(&label);
        Self { root, label }
    }

    fn set(&self, text: &str, active: bool) {
        self.label.set_text(text);
        if active {
            self.root.add_css_class("active");
        } else {
            self.root.remove_css_class("active");
        }
    }
}

pub struct TabStrip {
    widget: gtk4::Box,
    slots: Vec<Tab>,
    /// Shown only while a workspace outside `1..=SLOTS` is focused. `Super+1..5`
    /// cannot reach one, but a scratchpad or a scripted dispatch can, and a
    /// strip that shows no active tab at all reads as a broken strip.
    overflow: Tab,
}

impl TabStrip {
    pub fn new() -> Self {
        let widget = gtk4::Box::new(Orientation::Horizontal, 0);
        widget.add_css_class("tabs");

        let slots: Vec<Tab> = (1..=SLOTS)
            .map(|_| {
                let t = Tab::new();
                widget.append(&t.root);
                t
            })
            .collect();

        let overflow = Tab::new();
        overflow.root.set_visible(false);
        widget.append(&overflow.root);

        let strip = Self { widget, slots, overflow };
        strip.set_offline();
        strip
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.widget
    }

    /// No compositor to ask. This is what the bar shows when it is run outside
    /// a Hyprland session — eDEX's own initial state, rather than blank tabs
    /// that look like a rendering failure.
    pub fn set_offline(&self) {
        for (i, tab) in self.slots.iter().enumerate() {
            tab.set(if i == 0 { "MAIN SHELL" } else { "EMPTY" }, i == 0);
        }
        self.overflow.root.set_visible(false);
    }

    pub fn apply(&self, snap: &Snapshot) {
        for (i, tab) in self.slots.iter().enumerate() {
            let id = i as i32 + 1;
            tab.set(&tab_label(snap.workspace(id)), snap.active == id);
        }

        let outside = !(1..=SLOTS).contains(&snap.active);
        self.overflow.root.set_visible(outside);
        if outside {
            self.overflow
                .set(&tab_label(snap.workspace(snap.active)), true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: i32, name: &str, windows: u32) -> Workspace {
        Workspace { id, name: name.into(), windows }
    }

    #[test]
    fn an_unnamed_workspace_gets_edex_vocabulary() {
        assert_eq!(tab_label(Some(&ws(1, "1", 2))), "MAIN SHELL");
        assert_eq!(tab_label(Some(&ws(3, "3", 1))), "SHELL 3");
    }

    #[test]
    fn a_named_workspace_keeps_its_name() {
        assert_eq!(tab_label(Some(&ws(4, "code", 1))), "CODE");
        assert_eq!(tab_label(Some(&ws(-99, "special:magic", 1))), "MAGIC");
    }

    #[test]
    fn empty_and_absent_read_the_same() {
        // Hyprland drops a workspace from `j/workspaces` the moment its last
        // window closes, so "exists with no windows" and "not there" are the
        // same state to a user and must not look different on the bar.
        assert_eq!(tab_label(Some(&ws(2, "2", 0))), "EMPTY");
        assert_eq!(tab_label(None), "EMPTY");
    }
}
