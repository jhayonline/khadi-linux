use smithay::{
    delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{
        PopupKeyboardGrab, PopupKind, PopupManager, PopupPointerGrab, PopupUngrabStrategy, Window,
        find_popup_root_surface, get_popup_toplevel_coords,
    },
    input::{Seat, pointer::Focus as PointerGrabFocus},
    reexports::{
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode,
            shell::server::xdg_toplevel,
        },
        wayland_server::{
            Resource,
            protocol::{wl_seat, wl_surface::WlSurface},
        },
    },
    utils::Serial,
    wayland::shell::xdg::{
        PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        decoration::XdgDecorationHandler,
    },
};

use crate::{EdexComp, focus::Focus};

impl XdgShellHandler for EdexComp {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // The shell is recognised by being the process this compositor started.
        let pid = surface
            .wl_surface()
            .client()
            .and_then(|client| client.get_credentials(&self.display_handle).ok())
            .map(|credentials| credentials.pid as u32);
        let from_shell = pid.is_some() && pid == self.shell_pid;
        self.add_window(Window::new_wayland_window(surface), from_shell);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.remove_window(surface.wl_surface());
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    // A video going full screen takes its whole display, frame included.
    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            let screen = self.screen_of(&window);
            if self.apps.iter().any(|app| app.window == window) {
                self.set_fullscreen(screen, true);
            }
        }
        // The request must be answered even when nothing changed.
        if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            let screen = self.screen_of(&window);
            self.set_fullscreen(screen, false);
        }
        if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    // Windows are placed by the compositor; clients cannot move or resize themselves.
    fn move_request(&mut self, _surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn resize_request(
        &mut self,
        _surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        _serial: Serial,
        _edges: xdg_toplevel::ResizeEdge,
    ) {
    }

    // Menus take the keyboard and pointer while open, so that a click elsewhere closes them.
    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::<EdexComp>::from_resource(&seat) else {
            return;
        };
        let kind = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        let Ok(mut grab) = self.popups.grab_popup(Focus::Surface(root), kind, &seat, serial) else {
            return;
        };
        if let Some(keyboard) = seat.get_keyboard() {
            if keyboard.is_grabbed()
                && !(keyboard.has_grab(serial)
                    || keyboard.has_grab(grab.previous_serial().unwrap_or(serial)))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            keyboard.set_focus(self, grab.current_grab(), serial);
            keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
        }
        if let Some(pointer) = seat.get_pointer() {
            if pointer.is_grabbed()
                && !(pointer.has_grab(serial)
                    || pointer.has_grab(grab.previous_serial().unwrap_or_else(|| grab.serial())))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, PointerGrabFocus::Keep);
        }
    }
}

// Applications are told not to draw their own title bars where they can avoid it: the
// frame already says what is open, and windows cannot be moved.
impl XdgDecorationHandler for EdexComp {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| state.decoration_mode = Some(DecorationMode::ServerSide));
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        toplevel.with_pending_state(|state| state.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|state| state.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
}
delegate_xdg_decoration!(EdexComp);

// Xdg Shell
delegate_xdg_shell!(EdexComp);

/// Should be called on `WlSurface::commit`. `window` is the window owning `surface`, if any.
pub fn handle_commit(popups: &mut PopupManager, window: Option<&Window>, surface: &WlSurface) {
    // Handle toplevel commits.
    if let Some(toplevel) = window.and_then(|w| w.toplevel()) {
        if !toplevel.is_initial_configure_sent() {
            toplevel.send_configure();
        }
    }

    // Handle popup commits.
    popups.commit(surface);
    if let Some(popup) = popups.find_popup(surface) {
        match popup {
            PopupKind::Xdg(ref xdg) => {
                if !xdg.is_initial_configure_sent() {
                    // NOTE: This should never fail as the initial configure is always
                    // allowed.
                    xdg.send_configure().expect("initial configure failed");
                }
            }
            PopupKind::InputMethod(ref _input_method) => {}
        }
    }
}

impl EdexComp {
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self.window_for(&root) else {
            return;
        };
        let Some(window_geo) = self.space.element_geometry(&window) else {
            return;
        };

        // Popups may use the whole display their window is on. The target geometry for
        // the positioner is relative to its parent's geometry.
        let Some(mut target) = self.screens.get(self.screen_of(&window)).copied() else {
            return;
        };
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;

        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}
