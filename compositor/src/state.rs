use std::{
    ffi::OsString,
    path::PathBuf,
    process::Child,
    sync::Arc,
    time::{Duration, Instant},
};

use smithay::{
    backend::{renderer::gles::GlesRenderer, winit::WinitGraphicsBackend},
    desktop::{PopupManager, Space, Window, WindowSurfaceType},
    input::{Seat, SeatState, pointer::CursorImageStatus},
    reexports::{
        calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_server::{
            Display, DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point, Rectangle},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        dmabuf::{DmabufGlobal, DmabufState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        session_lock::SessionLockManagerState,
        shell::xdg::{XdgShellState, decoration::XdgDecorationState},
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};

use crate::{
    ipc::IpcServer,
    lock::Lock,
    policy::{App, ScreenMode},
    udev::UdevData,
};
use khadi_common::{Panels, ipc::Place};
use smithay::{reexports::calloop::LoopHandle, wayland::xwayland_shell::XWaylandShellState, xwayland::X11Wm};

/// How to start the shell.
pub struct ShellSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// The shell may crash this often within [`CRASH_WINDOW`] before the session gives up.
const MAX_CRASHES: usize = 3;
const CRASH_WINDOW: Duration = Duration::from_secs(30);

pub struct EdexComp {
    pub start_time: std::time::Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,

    pub space: Space<Window>,
    pub loop_signal: LoopSignal,
    pub loop_handle: LoopHandle<'static, EdexComp>,
    /// The windowed backend, when running inside another desktop.
    pub backend: Option<WinitGraphicsBackend<GlesRenderer>>,
    /// The hardware backend, when running as a login session.
    pub udev: Option<UdevData>,

    // Smithay State
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub xwayland_shell_state: XWaylandShellState,
    /// The X11 window manager, once the X server is up.
    pub xwm: Option<X11Wm>,
    /// The X server's display number, for `DISPLAY`.
    pub x_display: Option<u32>,
    /// X11 menus, tooltips and the like: windows that place themselves.
    pub x_popups: Vec<Window>,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<EdexComp>,
    pub data_device_state: DataDeviceState,
    pub session_lock_state: SessionLockManagerState,
    /// Set while the session is locked. See `lock.rs`.
    pub lock: Option<Lock>,
    pub dmabuf_state: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,
    pub popups: PopupManager,

    pub seat: Seat<Self>,

    // Desktop policy
    pub shell_spec: Option<ShellSpec>,
    pub shell_child: Option<Child>,
    /// When the shell last died without being asked to.
    shell_crashes: Vec<Instant>,
    /// Set while a restart of the shell was asked for and its old process is going away.
    shell_restarting: bool,
    pub shell_pid: Option<u32>,
    /// The khadi shell window: always fullscreen, always at the bottom.
    pub shell: Option<Window>,
    /// The shell's backdrop windows: the nth covers display n + 1 behind its applications.
    pub backdrops: Vec<Window>,
    /// Application windows, in the order they were opened.
    pub apps: Vec<App>,
    /// The displays, in logical coordinates; the first is the one with the frame.
    pub screens: Vec<Rectangle<i32, Logical>>,
    /// How each display's workspace is being used; parallel to `screens`.
    pub modes: Vec<ScreenMode>,
    /// Which of the frame's panels are open.
    pub panels: Panels,
    /// The display in use: its application has the keyboard, and shortcuts act on it.
    pub focus_screen: usize,
    /// What the client under the pointer wants the pointer to look like.
    pub cursor_status: CursorImageStatus,
    /// Testing aid: treat the left and right halves of the window as two displays.
    pub split: bool,

    pub ipc: Option<IpcServer>,
    /// Set when the compositor must exit with an error.
    pub fatal: Option<String>,
    /// A display layout chosen in the settings and not yet saved, which overrides
    /// the saved ones.
    /// The keyboard, mouse and touchpad settings in force.
    pub input_settings: khadi_common::input::InputSettings,
    /// The keyboard layout and variant last given to the keymap compiler.
    pub keymap_applied: Option<(String, String)>,
    pub display_override: Option<Vec<crate::monitors::Saved>>,
    /// Set while a new layout waits to be confirmed: what to go back to, and since when.
    pub display_pending: Option<(Option<Vec<crate::monitors::Saved>>, Instant)>,
    pub screenshot: Option<PathBuf>,
}

impl EdexComp {
    pub fn new(event_loop: &mut EventLoop<'static, EdexComp>, display: Display<Self>) -> Self {
        let start_time = std::time::Instant::now();

        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        // Any client may lock the session. Restricting this to one trusted program
        // would need a way to tell them apart that Khadi does not have yet, and the
        // protocol's own guarantees do not depend on who asks.
        let session_lock_state = SessionLockManagerState::new::<Self, _>(&dh, |_| true);
        let popups = PopupManager::default();

        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "winit");
        seat.add_keyboard(Default::default(), 200, 25).unwrap();
        seat.add_pointer();

        let space = Space::default();
        let socket_name = Self::init_wayland_listener(display, event_loop);
        let loop_signal = event_loop.get_signal();

        Self {
            start_time,
            display_handle: dh.clone(),

            space,
            loop_signal,
            loop_handle: event_loop.handle(),
            socket_name,
            backend: None,
            udev: None,

            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            xwayland_shell_state: XWaylandShellState::new::<Self>(&dh),
            xwm: None,
            x_display: None,
            x_popups: Vec::new(),
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            session_lock_state,
            lock: None,
            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            popups,
            seat,

            shell_spec: None,
            shell_child: None,
            shell_crashes: Vec::new(),
            shell_restarting: false,
            shell_pid: None,
            shell: None,
            backdrops: Vec::new(),
            apps: Vec::new(),
            screens: Vec::new(),
            modes: Vec::new(),
            panels: Panels::default(),
            focus_screen: 0,
            cursor_status: CursorImageStatus::default_named(),
            split: false,

            ipc: None,
            fatal: None,
            input_settings: khadi_common::input::InputSettings::default(),
            keymap_applied: None,
            display_override: None,
            display_pending: None,
            screenshot: None,
        }
    }

    fn init_wayland_listener(
        display: Display<EdexComp>,
        event_loop: &mut EventLoop<'static, EdexComp>,
    ) -> OsString {
        // Creates a new listening socket, automatically choosing the next available `wayland` socket name.
        let listening_socket = ListeningSocketSource::new_auto().unwrap();
        let socket_name = listening_socket.socket_name().to_os_string();

        let loop_handle = event_loop.handle();

        loop_handle
            .insert_source(listening_socket, move |client_stream, _, state| {
                state
                    .display_handle
                    .insert_client(client_stream, Arc::new(ClientState::default()))
                    .unwrap();
            })
            .expect("Failed to init the wayland event source.");

        // The display itself is an event source, so that client requests get dispatched.
        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // Safety: we don't drop the display
                    unsafe {
                        display.get_mut().dispatch_clients(state).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .unwrap();

        socket_name
    }

    pub fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        // While locked the pointer can only be over the lock screen. Without this the
        // desktop underneath would still take clicks and hovers, and would still say
        // what the pointer should look like over it.
        if let Some(lock) = self.lock.as_ref() {
            let output = self
                .space
                .outputs()
                .find(|output| {
                    self.space
                        .output_geometry(output)
                        .is_some_and(|geometry| geometry.to_f64().contains(pos))
                })
                .cloned()?;
            let origin = self.space.output_geometry(&output)?.loc.to_f64();
            let surface = lock.surface_for(&output)?.wl_surface().clone();
            return Some((surface, origin));
        }
        self.space.element_under(pos).and_then(|(window, location)| {
            window
                .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + location).to_f64()))
        })
    }

    /// The renderer of whichever backend is running.
    pub fn renderer(&mut self) -> Option<&mut GlesRenderer> {
        match (&mut self.backend, &mut self.udev) {
            (Some(winit), _) => Some(winit.renderer()),
            (None, Some(udev)) => Some(&mut udev.renderer),
            (None, None) => None,
        }
    }

    /// Work due before drawing a frame. Returns false once the session is over.
    pub fn before_frame(&mut self) -> bool {
        if let Some(child) = self.shell_child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                self.shell_child = None;
                let restarting = std::mem::take(&mut self.shell_restarting);
                // The session ends when the user quits the shell. A shell that crashed
                // is started again instead: the applications are still running.
                if status.success() && !restarting {
                    tracing::info!("shell exited, ending session");
                    self.loop_signal.stop();
                    return false;
                }
                if !restarting {
                    tracing::error!(%status, "shell crashed, starting it again");
                    self.shell_crashes.retain(|at| at.elapsed() < CRASH_WINDOW);
                    self.shell_crashes.push(Instant::now());
                }
                let result = if self.shell_crashes.len() > MAX_CRASHES {
                    Err("the shell keeps crashing".to_string())
                } else {
                    self.spawn_shell().map_err(|e| e.to_string())
                };
                if let Err(reason) = result {
                    self.fatal = Some(reason);
                    self.loop_signal.stop();
                    return false;
                }
            }
        }
        // A display change nobody confirmed is taken back: the screen may be unreadable.
        if self.display_pending.as_ref().is_some_and(|(_, since)| since.elapsed() > crate::displays::CONFIRM_WITHIN) {
            self.revert_displays();
        }
        self.pump_ipc();
        true
    }

    /// Starts the shell, pointed at this compositor.
    pub fn spawn_shell(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(spec) = &self.shell_spec else {
            return Err("no shell configured".into());
        };
        let mut command = std::process::Command::new(&spec.program);
        if let Some(server) = &self.ipc {
            command.env(khadi_common::ipc::SOCKET_ENV, server.path());
        }
        // Only the shell is pointed at our socket; programs started from it inherit it.
        // Never a host desktop's X server: programs must open here.
        match self.x_display {
            Some(number) => command.env("DISPLAY", format!(":{number}")),
            None => command.env_remove("DISPLAY"),
        };
        let child = command
            .args(&spec.args)
            .env("WAYLAND_DISPLAY", &self.socket_name)
            // Programs should pick their Wayland backends where that is not yet their
            // default; those that only speak X11 get our own X server.
            .env("XDG_SESSION_TYPE", "wayland")
            .env("MOZ_ENABLE_WAYLAND", "1")
            .env("QT_QPA_PLATFORM", "wayland;xcb")
            .env("SDL_VIDEODRIVER", "wayland,x11")
            .env("ELECTRON_OZONE_PLATFORM_HINT", "auto")
            .spawn()
            .map_err(|e| format!("cannot start shell {}: {e}", spec.program.display()))?;
        tracing::info!(pid = child.id(), shell = %spec.program.display(), "started shell");
        self.shell_pid = Some(child.id());
        self.shell_child = Some(child);
        Ok(())
    }

    /// Replaces the running shell with a fresh one, which picks up a newly installed
    /// build. Its terminal tabs go with it; applications are untouched.
    pub fn restart_shell(&mut self) {
        if let Some(child) = self.shell_child.as_mut() {
            tracing::info!("restarting the shell");
            self.shell_restarting = true;
            let _ = child.kill();
        }
    }

    /// Work due after drawing a frame: let clients draw their next one, and tidy up.
    pub fn after_frame(&mut self) {
        let Some(output) = self.space.outputs().next().cloned() else {
            return;
        };
        let elapsed = self.start_time.elapsed();
        self.space.elements().for_each(|window| {
            window.send_frame(&output, elapsed, Some(Duration::ZERO), |_, _| Some(output.clone()))
        });
        // The shell draws all of its windows in one pass and waits for each to be shown.
        // A backdrop whose display has just gone is on no screen, and without this the
        // shell would wait on it forever, freezing the whole frame.
        for backdrop in &self.backdrops {
            backdrop.send_frame(&output, elapsed, Some(Duration::ZERO), |_, _| Some(output.clone()));
        }
        // Applications out of view are kept ticking slowly: enough that none waits
        // forever for a chance to draw, without having them all render at full rate.
        for app in self.apps.iter().filter(|app| app.place == Place::Hidden) {
            app.window
                .send_frame(&output, elapsed, Some(Duration::from_secs(1)), |_, _| None);
        }
        self.space.refresh();
        self.popups.cleanup();
    }

}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
