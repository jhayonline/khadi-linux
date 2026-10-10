//! Locking the session, through `ext-session-lock-v1`.
//!
//! The protocol puts the compositor in charge of the one guarantee that matters:
//! while the session is locked, nothing drawn before the lock may be shown and no
//! input may reach it. The program that draws the lock screen and checks the
//! password is an ordinary client with no special powers — if it crashes the screen
//! stays black and locked rather than falling open. That is why this is done here
//! and not with a window that tries to stay on top of the others.
//!
//! Khadi has no lock screen of its own yet. Any client speaking this protocol works,
//! `swaylock` among them, which is what made this testable before there was one.
//!
//! ## If the lock screen crashes
//!
//! The session stays locked and every display goes black, which is the safe
//! direction and what the protocol asks for. Getting back in means switching to
//! another virtual terminal (Ctrl+Alt+F2) and ending the compositor from there,
//! which loses the session.
//!
//! A better recovery would be to let a second lock client take over the black
//! screen, which is what sway does. Smithay 0.7 does not allow it: the manager
//! records which outputs are locked in a field private to the crate and only clears
//! it on an orderly unlock, so a replacement client asking for the same output is
//! killed with a protocol error. Worth revisiting if that field ever opens up.

use std::collections::{HashMap, HashSet};

use smithay::{
    delegate_session_lock,
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    utils::SERIAL_COUNTER,
    wayland::session_lock::{LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker},
};

use crate::{EdexComp, focus::Focus};

/// A locked session.
#[derive(Default)]
pub struct Lock {
    /// Held until a frame with no client content has been shown on every display.
    /// Only then is the locking client told the lock took effect: before that it is
    /// still answerable for what is on the screen, and confirming early is how a
    /// lock screen comes up with the desktop visible behind it for a frame or two.
    pending: Option<SessionLocker>,
    /// The locker's surface for each display, by output name. A display the locker
    /// has not covered stays black — with nothing to show, showing nothing is the
    /// only safe thing to do.
    surfaces: HashMap<String, LockSurface>,
    /// Displays that have presented a frame since the lock began.
    blanked: HashSet<String>,
}

impl Lock {
    /// What to draw on this display, if anything.
    pub fn surface_for(&self, output: &Output) -> Option<&LockSurface> {
        self.surfaces.get(&output.name())
    }
}

impl EdexComp {
    /// True while the session is locked: nothing underneath may be drawn or typed into.
    pub fn locked(&self) -> bool {
        self.lock.is_some()
    }

    /// Records that this display has shown a frame under the lock, and confirms the
    /// lock to the client once every display has.
    ///
    /// Called after presenting, not before. "We have stopped drawing the desktop" and
    /// "the desktop is no longer on the screen" are different claims, and the client
    /// is owed the second one.
    pub fn lock_frame_presented(&mut self, output: &Output) {
        let outputs: Vec<String> = self.space.outputs().map(Output::name).collect();
        let Some(lock) = self.lock.as_mut() else {
            return;
        };
        if lock.pending.is_none() {
            return;
        }
        lock.blanked.insert(output.name());
        if !outputs.iter().all(|name| lock.blanked.contains(name)) {
            return;
        }
        if let Some(confirmation) = lock.pending.take() {
            tracing::info!("session locked");
            confirmation.lock();
        }
    }

    /// Hands the keyboard to the lock screen, or to nothing at all when it has not
    /// put a surface up yet. Never to a window underneath.
    pub(crate) fn focus_lock(&mut self) {
        let focus = self
            .lock
            .as_ref()
            .and_then(|lock| lock.surfaces.values().next())
            .map(|surface| Focus::Surface(surface.wl_surface().clone()));
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, focus, SERIAL_COUNTER.next_serial());
    }

    /// A surface has gone. While locked, this is how a lock screen that crashed is
    /// noticed: its display is dropped from the map and from then on draws black.
    /// It deliberately does not unlock.
    pub fn lock_surface_gone(&mut self, surface: &WlSurface) {
        let Some(lock) = self.lock.as_mut() else {
            return;
        };
        let gone: Vec<String> = lock
            .surfaces
            .iter()
            .filter(|(_, candidate)| candidate.wl_surface() == surface)
            .map(|(name, _)| name.clone())
            .collect();
        if gone.is_empty() {
            return;
        }
        for name in gone {
            tracing::warn!(display = %name, "the lock screen is gone; this display stays black and locked");
            lock.surfaces.remove(&name);
        }
        self.focus_lock();
    }
}

impl SessionLockHandler for EdexComp {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock_state
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        tracing::info!("locking the session");
        self.lock = Some(Lock {
            pending: Some(confirmation),
            ..Default::default()
        });
        // Take the keyboard away from whatever holds it before the first lock surface
        // arrives, so the gap between asking to lock and being locked is not a way in.
        self.focus_lock();
    }

    fn unlock(&mut self) {
        tracing::info!("unlocking the session");
        self.lock = None;
        // Puts the windows back and gives the keyboard to whatever was in use.
        self.arrange();
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else {
            tracing::warn!("lock surface for a display that is not ours");
            return;
        };
        // The locker gets the whole display, and no say in it.
        let size = self
            .space
            .output_geometry(&output)
            .map(|geometry| geometry.size)
            .unwrap_or_default();
        surface.with_pending_state(|state| {
            state.size = Some((size.w.max(0) as u32, size.h.max(0) as u32).into());
        });
        surface.send_configure();

        let name = output.name();
        tracing::info!(display = %name, "lock screen covering this display");
        if let Some(lock) = self.lock.as_mut() {
            lock.surfaces.insert(name, surface);
        }
        self.focus_lock();
    }
}

delegate_session_lock!(EdexComp);
