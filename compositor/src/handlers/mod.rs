mod compositor;
mod xdg_shell;

use crate::{EdexComp, focus::Focus};
use smithay::wayland::seat::WaylandFocus;

//
// Wl Seat
//

use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::{SelectionHandler, SelectionSource, SelectionTarget};
use smithay::{delegate_data_device, delegate_output, delegate_seat};

impl SeatHandler for EdexComp {
    type KeyboardFocus = Focus;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<EdexComp> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: smithay::input::pointer::CursorImageStatus) {
        self.cursor_status = image;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&Focus>) {
        let dh = &self.display_handle;
        let client = focused
            .and_then(|focus| focus.wl_surface())
            .and_then(|surface| dh.get_client(surface.id()).ok());
        set_data_device_focus(dh, seat, client);
    }
}

delegate_seat!(EdexComp);

//
// Wl Data Device
//

impl SelectionHandler for EdexComp {
    type SelectionUserData = ();

    // What a Wayland application copies is offered to X11 applications too.
    fn new_selection(&mut self, ty: SelectionTarget, source: Option<SelectionSource>, _seat: Seat<Self>) {
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(e) = xwm.new_selection(ty, source.map(|source| source.mime_types())) {
                tracing::warn!("cannot offer the clipboard to X11 applications: {e}");
            }
        }
    }

    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: std::os::fd::OwnedFd,
        _seat: Seat<Self>,
        _user_data: &(),
    ) {
        if let Some(xwm) = self.xwm.as_mut() {
            if let Err(e) = xwm.send_selection(ty, mime_type, fd, self.loop_handle.clone()) {
                tracing::warn!("cannot fetch the clipboard from an X11 application: {e}");
            }
        }
    }
}

impl DataDeviceHandler for EdexComp {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for EdexComp {}
impl ServerDndGrabHandler for EdexComp {}

delegate_data_device!(EdexComp);

//
// Wl Output & Xdg Output
//

impl OutputHandler for EdexComp {}
delegate_output!(EdexComp);
