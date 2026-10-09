//! What can hold the keyboard: a Wayland surface, or an X11 window.
//!
//! X11 windows are drawn through a Wayland surface too, but the X server keeps its own
//! idea of which window has the keyboard and must be told as well.

use std::borrow::Cow;

use smithay::{
    backend::input::KeyState,
    desktop::PopupKind,
    input::{
        Seat,
        keyboard::{KeyboardTarget, KeysymHandle, ModifiersState},
    },
    reexports::wayland_server::{backend::ObjectId, protocol::wl_surface::WlSurface},
    utils::{IsAlive, Serial},
    wayland::seat::WaylandFocus,
    xwayland::X11Surface,
};

use crate::EdexComp;

#[derive(Debug, Clone, PartialEq)]
pub enum Focus {
    Surface(WlSurface),
    X11(X11Surface),
}

impl IsAlive for Focus {
    fn alive(&self) -> bool {
        match self {
            Focus::Surface(surface) => surface.alive(),
            Focus::X11(window) => window.alive(),
        }
    }
}

impl WaylandFocus for Focus {
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        match self {
            Focus::Surface(surface) => Some(Cow::Borrowed(surface)),
            Focus::X11(window) => window.wl_surface().map(Cow::Owned),
        }
    }

    fn same_client_as(&self, object_id: &ObjectId) -> bool {
        match self {
            Focus::Surface(surface) => surface.same_client_as(object_id),
            Focus::X11(window) => window.same_client_as(object_id),
        }
    }
}

impl KeyboardTarget<EdexComp> for Focus {
    fn enter(&self, seat: &Seat<EdexComp>, data: &mut EdexComp, keys: Vec<KeysymHandle<'_>>, serial: Serial) {
        match self {
            Focus::Surface(surface) => KeyboardTarget::enter(surface, seat, data, keys, serial),
            Focus::X11(window) => KeyboardTarget::enter(window, seat, data, keys, serial),
        }
    }

    fn leave(&self, seat: &Seat<EdexComp>, data: &mut EdexComp, serial: Serial) {
        match self {
            Focus::Surface(surface) => KeyboardTarget::leave(surface, seat, data, serial),
            Focus::X11(window) => KeyboardTarget::leave(window, seat, data, serial),
        }
    }

    fn key(
        &self,
        seat: &Seat<EdexComp>,
        data: &mut EdexComp,
        key: KeysymHandle<'_>,
        state: KeyState,
        serial: Serial,
        time: u32,
    ) {
        match self {
            Focus::Surface(surface) => KeyboardTarget::key(surface, seat, data, key, state, serial, time),
            Focus::X11(window) => KeyboardTarget::key(window, seat, data, key, state, serial, time),
        }
    }

    fn modifiers(&self, seat: &Seat<EdexComp>, data: &mut EdexComp, modifiers: ModifiersState, serial: Serial) {
        match self {
            Focus::Surface(surface) => KeyboardTarget::modifiers(surface, seat, data, modifiers, serial),
            Focus::X11(window) => KeyboardTarget::modifiers(window, seat, data, modifiers, serial),
        }
    }
}

impl From<PopupKind> for Focus {
    fn from(popup: PopupKind) -> Self {
        Focus::Surface(popup.wl_surface().clone())
    }
}

/// Used when a menu's grab ends and the pointer returns to what the grab started on,
/// which is always a Wayland surface: X11 menus do not go through such grabs.
impl From<Focus> for WlSurface {
    fn from(focus: Focus) -> Self {
        match focus {
            Focus::Surface(surface) => surface,
            Focus::X11(window) => window
                .wl_surface()
                .expect("an X11 window cannot be the root of a Wayland popup grab"),
        }
    }
}
