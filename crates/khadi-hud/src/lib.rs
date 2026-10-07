//! The Khadi terminal surface, as a library.
//!
//! `khadi-hud` is both a binary and a lib because the login screen needs the
//! same chrome the panels use. `khadi-greet` draws the bracket-tick header and
//! the block clock, and a second implementation of either would be a second
//! thing to keep in step with the design — the exact mistake `khadi-core`
//! exists to prevent on the data side.
//!
//! The panels stay here rather than moving to a neutral crate: nothing outside
//! this binary draws a process list, and a `khadi-tui` crate holding two
//! widgets would be ceremony.
pub mod dash;
pub mod fs_panel;
pub mod globe;
pub mod net_panel;
pub mod widgets;
