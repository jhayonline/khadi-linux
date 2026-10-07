//! Khadi core — theme and metrics, with no UI dependencies.
//!
//! Two surfaces consume this: the ratatui dashboard (Phase 3a) and the GTK
//! layer-shell panel (Phase 3b). Anything that knows about a cell grid or a
//! widget tree belongs in khadi-hud, not here.
pub mod disks;
pub mod hypr;
pub mod metrics;
pub mod net;
pub mod theme;

pub use theme::{Rgb, Theme};
