//! The hardware backend: runs the compositor directly on displays and input devices,
//! as a login session, instead of inside another desktop's window.
//!
//! It drives the boot GPU and every monitor connected to it, including ones plugged in
//! later. Modes and arrangement follow the layout saved in GNOME where there is one;
//! otherwise a laptop's own panel is the main display and the others extend to its right.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode,
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLContext, EGLDevice, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            ImportDma,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                render_elements,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::GlesRenderer,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, primary_gpu},
    },
    desktop::space::{SpaceRenderElements, space_render_elements},
    input::pointer::{CursorImageAttributes, CursorImageStatus},
    output::{Mode as WlMode, Output, PhysicalProperties},
    reexports::{
        calloop::{
            EventLoop,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{Device as ControlDevice, ModeFlags, ModeTypeFlags, connector, crtc},
        input::{self, Libinput},
        rustix::fs::OFlags,
        wayland_server::backend::GlobalId,
    },
    utils::{DeviceFd, IsAlive, Rectangle, Transform},
    wayland::{compositor::with_states, dmabuf::DmabufFeedbackBuilder},
};

use crate::{EdexComp, monitors};
use khadi_common::ipc::{DisplayInfo, Mode as DisplayMode};

/// How long the first frame may take to reach a screen before the backend gives up.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

type Manager =
    DrmOutputManager<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;
type Surface = DrmOutput<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

render_elements! {
    pub OutputElements<=GlesRenderer>;
    Space=SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
    Cursor=MemoryRenderBufferRenderElement<GlesRenderer>,
    ClientCursor=WaylandSurfaceRenderElement<GlesRenderer>,
}

/// One monitor and the means to draw on it.
struct Monitor {
    connector: connector::Handle,
    crtc: crtc::Handle,
    /// A laptop's built-in panel, which is preferred as the main display.
    internal: bool,
    surface: Surface,
    output: Output,
    global: GlobalId,
    refresh: Duration,
    /// The modes it can run at, best first.
    modes: Vec<DisplayMode>,
    /// When the frame currently waiting for its page flip was queued.
    pending_since: Option<Instant>,
    /// Whether a frame has reached this monitor yet.
    lit: bool,
}

pub struct UdevData {
    pub renderer: GlesRenderer,
    /// Connected monitors, the main display first.
    monitors: Vec<Monitor>,
    manager: Manager,
    cursor: MemoryRenderBuffer,
    /// False while another virtual terminal owns the displays.
    active: bool,
    /// Frames that have reached a screen.
    flips: u64,
    /// The input devices present, so that settings can be applied to them later.
    devices: Vec<input::Device>,
    // Declared last so that it is dropped last: the devices above were opened through it.
    pub session: LibSeatSession,
}

/// The pointer, top-left corner at the hotspot. `X` is the outline, `.` the fill.
const CURSOR_SHAPE: [&str; 17] = [
    "X           ",
    "XX          ",
    "X.X         ",
    "X..X        ",
    "X...X       ",
    "X....X      ",
    "X.....X     ",
    "X......X    ",
    "X.......X   ",
    "X........X  ",
    "X.........X ",
    "X......XXXXX",
    "X...X..X    ",
    "X..XX..X    ",
    "X.X  X..X   ",
    "XX   X..X   ",
    "X     XX    ",
];

fn cursor_buffer() -> MemoryRenderBuffer {
    let width = CURSOR_SHAPE[0].len();
    let mut data = Vec::with_capacity(width * CURSOR_SHAPE.len() * 4);
    for row in CURSOR_SHAPE {
        for cell in row.bytes() {
            // Argb8888 is stored as B, G, R, A.
            data.extend_from_slice(match cell {
                b'X' => &[0, 0, 0, 255],
                b'.' => &[255, 255, 255, 255],
                _ => &[0, 0, 0, 0],
            });
        }
    }
    MemoryRenderBuffer::from_slice(
        &data,
        Fourcc::Argb8888,
        (width as i32, CURSOR_SHAPE.len() as i32),
        1,
        Transform::Normal,
        None,
    )
}

pub fn init_udev(
    event_loop: &mut EventLoop<'static, EdexComp>,
    state: &mut EdexComp,
) -> Result<(), Box<dyn std::error::Error>> {
    let display_handle = &state.display_handle.clone();

    // The session grants access to the display and input devices, and takes it away
    // again while the user is on another virtual terminal.
    let (mut session, session_notifier) = LibSeatSession::new()?;
    let seat_name = session.seat();

    let gpu_path = primary_gpu(&seat_name)?.ok_or("no GPU found for this seat")?;
    tracing::info!(gpu = %gpu_path.display(), seat = %seat_name, "starting on hardware");
    let fd = session.open(
        &gpu_path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd)?;

    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    let render_node = EGLDevice::device_for_display(&egl_display)
        .ok()
        .and_then(|device| device.try_get_render_node().ok().flatten())
        .or_else(|| DrmNode::from_path(&gpu_path).ok());
    let egl_context = EGLContext::new(&egl_display)?;
    let renderer = unsafe { GlesRenderer::new(egl_context)? };

    let render_formats = renderer.egl_context().dmabuf_render_formats().clone();
    let manager: Manager = DrmOutputManager::new(
        drm,
        GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT),
        GbmFramebufferExporter::new(gbm.clone(), render_node),
        Some(gbm),
        [Fourcc::Abgr8888, Fourcc::Argb8888],
        render_formats,
    );

    // Let clients hand over GPU buffers (see the winit backend for why feedback matters).
    let formats = renderer.dmabuf_formats();
    let feedback =
        render_node.and_then(|node| DmabufFeedbackBuilder::new(node.dev_id(), formats.clone()).build().ok());
    state.dmabuf_global = Some(match &feedback {
        Some(feedback) => state
            .dmabuf_state
            .create_global_with_default_feedback::<EdexComp>(display_handle, feedback),
        None => state.dmabuf_state.create_global::<EdexComp>(display_handle, formats),
    });

    state.udev = Some(UdevData {
        renderer,
        monitors: Vec::new(),
        manager,
        cursor: cursor_buffer(),
        active: true,
        flips: 0,
        devices: Vec::new(),
        session: session.clone(),
    });
    state.rescan_monitors();
    if state.udev.as_ref().is_some_and(|udev| udev.monitors.is_empty()) {
        return Err("no connected monitor could be set up".into());
    }

    let mut libinput = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.into());
    libinput
        .udev_assign_seat(&seat_name)
        .map_err(|_| "cannot assign the seat to libinput")?;
    event_loop
        .handle()
        .insert_source(LibinputInputBackend::new(libinput.clone()), |mut event, _, data| {
            match &mut event {
                InputEvent::DeviceAdded { device } => {
                    configure_device(device, &data.input_settings);
                    if let Some(udev) = data.udev.as_mut() {
                        udev.devices.push(device.clone());
                    }
                }
                InputEvent::DeviceRemoved { device } => {
                    if let Some(udev) = data.udev.as_mut() {
                        udev.devices.retain(|known| known != device);
                    }
                }
                _ => {}
            }
            data.process_input_event(event);
        })?;

    event_loop
        .handle()
        .insert_source(session_notifier, move |event, _, data| {
            let Some(udev) = data.udev.as_mut() else {
                return;
            };
            match event {
                SessionEvent::PauseSession => {
                    tracing::info!("session paused");
                    libinput.suspend();
                    udev.manager.pause();
                    udev.active = false;
                }
                SessionEvent::ActivateSession => {
                    tracing::info!("session resumed");
                    if libinput.resume().is_err() {
                        tracing::error!("cannot resume input devices");
                    }
                    if let Err(e) = udev.manager.activate(false) {
                        tracing::error!("cannot take the displays back: {e}");
                    }
                    for monitor in &mut udev.monitors {
                        monitor.surface.reset_buffers();
                        monitor.pending_since = None;
                    }
                    udev.active = true;
                    // Monitors may have come or gone while we were away.
                    data.rescan_monitors();
                }
            }
        })?;

    event_loop
        .handle()
        .insert_source(drm_notifier, |event, _, data| match event {
            DrmEvent::VBlank(crtc) => {
                let Some(udev) = data.udev.as_mut() else {
                    return;
                };
                if let Some(monitor) = udev.monitors.iter_mut().find(|monitor| monitor.crtc == crtc) {
                    if let Err(e) = monitor.surface.frame_submitted() {
                        tracing::warn!("page flip bookkeeping failed: {e}");
                    }
                    monitor.pending_since = None;
                    if !std::mem::replace(&mut monitor.lit, true) {
                        tracing::info!(output = %monitor.output.name(), "first frame on screen");
                    }
                    udev.flips += 1;
                }
            }
            DrmEvent::Error(e) => tracing::error!("display error: {e}"),
        })?;

    // The kernel announces a plugged or unplugged monitor as a change to the GPU.
    event_loop
        .handle()
        .insert_source(UdevBackend::new(&seat_name)?, |_, _, data| {
            if data.udev.as_ref().is_some_and(|udev| udev.active) {
                data.rescan_monitors();
            }
        })?;

    // Frames are attempted once per refresh of the fastest monitor. Nothing is drawn or
    // queued for a monitor unless something on it changed, and never while its previous
    // frame is still waiting for its page flip.
    event_loop
        .handle()
        .insert_source(Timer::immediate(), move |_, _, data| {
            let state = data;
            if !state.before_frame() {
                return TimeoutAction::Drop;
            }
            // Give up rather than sit at a black screen: the session launcher can then
            // fall back to another way of starting the desktop.
            let stuck = state.udev.as_ref().is_some_and(|udev| udev.active && udev.flips == 0);
            if stuck && state.start_time.elapsed() > STARTUP_TIMEOUT {
                state.fatal = Some("no frame reached the screen".to_string());
                state.loop_signal.stop();
                return TimeoutAction::Drop;
            }
            state.render_udev();
            state.after_frame();
            let _ = state.display_handle.flush_clients();
            let frame_time = state
                .udev
                .as_ref()
                .and_then(|udev| udev.monitors.iter().map(|monitor| monitor.refresh).min())
                .unwrap_or(Duration::from_millis(16));
            TimeoutAction::ToDuration(frame_time)
        })?;

    Ok(())
}

/// Applies the settings to one input device: touchpads and mice each get their own.
fn configure_device(device: &mut input::Device, settings: &khadi_common::input::InputSettings) {
    // Only touchpads can tap.
    let touchpad = device.config_tap_finger_count() > 0;
    if touchpad {
        let _ = device.config_tap_set_enabled(settings.touchpad_tap);
        if device.config_dwt_is_available() {
            let _ = device.config_dwt_set_enabled(settings.touchpad_while_typing_off);
        }
    } else if !device.has_capability(input::DeviceCapability::Pointer) {
        return;
    }
    let (natural, speed) = if touchpad {
        (settings.touchpad_natural_scroll, settings.touchpad_speed)
    } else {
        (settings.mouse_natural_scroll, settings.mouse_speed)
    };
    if device.config_scroll_has_natural_scroll() {
        let _ = device.config_scroll_set_natural_scroll_enabled(natural);
    }
    if device.config_accel_is_available() {
        let _ = device.config_accel_set_speed(speed as f64);
    }
    if !touchpad && device.config_left_handed_is_available() {
        let _ = device.config_left_handed_set(settings.mouse_left_handed);
    }
}

/// The modes a monitor offers, without interlaced ones or repeats, largest and fastest
/// first.
fn offered_modes(info: &connector::Info) -> Vec<DisplayMode> {
    let mut modes: Vec<DisplayMode> = Vec::new();
    for mode in info.modes() {
        if mode.flags().contains(ModeFlags::INTERLACE) {
            continue;
        }
        let wl = WlMode::from(*mode);
        let offered = DisplayMode {
            width: wl.size.w as u32,
            height: wl.size.h as u32,
            refresh: wl.refresh as u32,
        };
        if offered.refresh > 0 && !modes.contains(&offered) {
            modes.push(offered);
        }
    }
    modes.sort_by_key(|mode| std::cmp::Reverse((mode.width * mode.height, mode.refresh)));
    modes.truncate(48);
    modes
}

/// The kernel's name for a connector, such as `HDMI-A-1`.
fn connector_name(info: &connector::Info) -> String {
    format!("{}-{}", info.interface().as_str(), info.interface_id())
}

impl EdexComp {
    /// Brings the set of monitors in line with what is plugged in, then lays them out.
    fn rescan_monitors(&mut self) {
        let Some(udev) = self.udev.as_mut() else {
            return;
        };
        let (resources, connected) = {
            let drm = udev.manager.device();
            let resources = match drm.resource_handles() {
                Ok(resources) => resources,
                Err(e) => {
                    tracing::warn!("cannot list display connectors: {e}");
                    return;
                }
            };
            let connected: Vec<connector::Info> = resources
                .connectors()
                .iter()
                .filter_map(|handle| drm.get_connector(*handle, true).ok())
                .filter(|info| info.state() == connector::State::Connected && !info.modes().is_empty())
                .collect();
            (resources, connected)
        };

        let names: Vec<String> = connected
            .iter()
            .map(|info| monitors::gnome_name(&connector_name(info)))
            .collect();
        // A layout just chosen in the settings comes before the saved ones.
        let saved = match &self.display_override {
            Some(layout) if layout.len() == names.len() && names.iter().all(|name| layout.iter().any(|saved| &saved.connector == name)) => {
                monitors::Layout {
                    monitors: layout.clone(),
                    arranged: true,
                }
            }
            _ => monitors::pick(&monitors::load_all(), &names),
        };

        let mut index = 0;
        while index < udev.monitors.len() {
            if connected.iter().any(|info| info.handle() == udev.monitors[index].connector) {
                index += 1;
                continue;
            }
            let gone = udev.monitors.remove(index);
            tracing::info!(output = %gone.output.name(), "monitor disconnected");
            self.space.unmap_output(&gone.output);
            self.display_handle.remove_global::<EdexComp>(gone.global.clone());
        }

        for info in &connected {
            if udev.monitors.iter().any(|monitor| monitor.connector == info.handle()) {
                continue;
            }
            let name = connector_name(info);
            // Each monitor needs a CRTC of its own, out of those its connector can use.
            let crtc = info
                .encoders()
                .iter()
                .filter_map(|encoder| udev.manager.device().get_encoder(*encoder).ok())
                .flat_map(|encoder| resources.filter_crtcs(encoder.possible_crtcs()))
                .find(|crtc| !udev.monitors.iter().any(|monitor| monitor.crtc == *crtc));
            let Some(crtc) = crtc else {
                tracing::warn!(output = %name, "no free CRTC for this monitor");
                continue;
            };
            // A mode the user already runs this monitor in beats the one it advertises as
            // its best, which the cable or the monitor may not actually manage.
            let wanted = saved.get(&monitors::gnome_name(&name));
            let drm_mode = wanted
                .and_then(|saved| {
                    info.modes()
                        .iter()
                        .filter(|mode| mode.size() == (saved.width as u16, saved.height as u16))
                        .filter(|mode| !mode.flags().contains(ModeFlags::INTERLACE))
                        .min_by_key(|mode| (WlMode::from(**mode).refresh as f64 - saved.rate * 1000.0).abs() as u64)
                })
                .or_else(|| {
                    info.modes()
                        .iter()
                        .find(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
                })
                .copied()
                .unwrap_or(info.modes()[0]);
            let wl_mode = WlMode::from(drm_mode);

            // Only the primary and cursor planes are used: every frame is composited by
            // the renderer, which keeps this backend simple.
            let mut planes = match udev.manager.device().planes(&crtc) {
                Ok(planes) => planes,
                Err(e) => {
                    tracing::warn!(output = %name, "cannot query planes: {e}");
                    continue;
                }
            };
            planes.overlay.clear();

            let (phys_w, phys_h) = info.size().unwrap_or((0, 0));
            let output = Output::new(
                name.clone(),
                PhysicalProperties {
                    size: (phys_w as i32, phys_h as i32).into(),
                    subpixel: info.subpixel().into(),
                    make: "Unknown".into(),
                    model: "Unknown".into(),
                },
            );
            output.set_preferred(wl_mode);
            output.change_current_state(Some(wl_mode), None, None, None);

            let surface = match udev.manager.initialize_output::<_, OutputElements>(
                crtc,
                drm_mode,
                &[info.handle()],
                &output,
                Some(planes),
                &mut udev.renderer,
                &DrmOutputRenderElements::default(),
            ) {
                Ok(surface) => surface,
                Err(e) => {
                    tracing::warn!(output = %name, "cannot set up this monitor: {e}");
                    continue;
                }
            };
            tracing::info!(output = %name, mode = ?wl_mode, from_saved_layout = wanted.is_some(), "monitor connected");
            udev.monitors.push(Monitor {
                modes: offered_modes(info),
                connector: info.handle(),
                crtc,
                internal: matches!(
                    info.interface(),
                    connector::Interface::EmbeddedDisplayPort
                        | connector::Interface::LVDS
                        | connector::Interface::DSI
                ),
                surface,
                global: output.create_global::<EdexComp>(&self.display_handle),
                output,
                refresh: Duration::from_secs_f64(1000.0 / wl_mode.refresh.max(1) as f64),
                pending_since: None,
                lit: false,
            });
        }

        // Arrange the monitors as saved, if what is saved describes exactly this set.
        let place = |monitor: &Monitor| saved.get(&monitors::gnome_name(&monitor.output.name()));
        let arranged = saved.arranged && udev.monitors.iter().all(|monitor| place(monitor).is_some());
        if arranged {
            udev.monitors
                .sort_by_key(|monitor| place(monitor).map(|saved| (!saved.primary, saved.x, saved.y)));
        } else {
            // The built-in panel leads; everything else follows it, left to right.
            udev.monitors.sort_by_key(|monitor| !monitor.internal);
        }
        let mut screens = Vec::new();
        let mut next_x = 0;
        for monitor in &udev.monitors {
            let Some(mode) = monitor.output.current_mode() else {
                continue;
            };
            let position = match place(monitor) {
                Some(saved) if arranged => (saved.x, saved.y),
                _ => (next_x, 0),
            };
            next_x += mode.size.w;
            monitor
                .output
                .change_current_state(None, None, None, Some(position.into()));
            self.space.map_output(&monitor.output, position);
            screens.push(Rectangle::new(position.into(), (mode.size.w, mode.size.h).into()));
        }
        self.set_screens(screens);
    }

    /// Applies the input settings in force to every device present.
    pub fn configure_devices(&mut self) {
        let settings = self.input_settings.clone();
        if let Some(udev) = self.udev.as_mut() {
            for device in &mut udev.devices {
                configure_device(device, &settings);
            }
        }
    }

    /// The monitors being driven, left to right; `None` when this backend is not in use.
    pub fn hardware_displays(&self) -> Option<Vec<DisplayInfo>> {
        let udev = self.udev.as_ref()?;
        let mut displays: Vec<DisplayInfo> = udev
            .monitors
            .iter()
            .enumerate()
            .filter_map(|(index, monitor)| {
                let mode = monitor.output.current_mode()?;
                let location = self.space.output_geometry(&monitor.output)?.loc;
                Some(DisplayInfo {
                    name: monitor.output.name(),
                    // The main display is kept first.
                    main: index == 0,
                    x: location.x,
                    y: location.y,
                    current: DisplayMode {
                        width: mode.size.w as u32,
                        height: mode.size.h as u32,
                        refresh: mode.refresh as u32,
                    },
                    modes: monitor.modes.clone(),
                })
            })
            .collect();
        displays.sort_by_key(|display| display.x);
        Some(displays)
    }

    /// Makes the monitors follow the layout now in force. A monitor whose mode has to
    /// change is set up afresh, the way a newly plugged one is.
    pub fn reapply_displays(&mut self) {
        let Some(udev) = self.udev.as_mut() else {
            return;
        };
        let wanted = self.display_override.clone().unwrap_or_default();
        let mut index = 0;
        while index < udev.monitors.len() {
            let monitor = &udev.monitors[index];
            let name = monitors::gnome_name(&monitor.output.name());
            let current = monitor.output.current_mode();
            let differs = wanted.iter().find(|saved| saved.connector == name).is_none_or(|saved| {
                current.is_none_or(|mode| {
                    (mode.size.w, mode.size.h) != (saved.width, saved.height)
                        || (mode.refresh as f64 - saved.rate * 1000.0).abs() > 1.0
                })
            });
            if differs {
                let gone = udev.monitors.remove(index);
                self.space.unmap_output(&gone.output);
                self.display_handle.remove_global::<EdexComp>(gone.global.clone());
            } else {
                index += 1;
            }
        }
        self.rescan_monitors();
    }

    fn render_udev(&mut self) {
        let Some(udev) = self.udev.as_mut() else {
            return;
        };
        if !udev.active {
            return;
        }
        if matches!(&self.cursor_status, CursorImageStatus::Surface(surface) if !surface.alive()) {
            self.cursor_status = CursorImageStatus::default_named();
        }
        let pointer = self.seat.get_pointer().unwrap().current_location();
        let UdevData {
            renderer,
            monitors,
            cursor,
            ..
        } = udev;

        for monitor in monitors.iter_mut() {
            // A page flip that never completes must not freeze the screen for good.
            match monitor.pending_since {
                Some(since) if since.elapsed() < Duration::from_secs(1) => continue,
                Some(_) => tracing::warn!("page flip timed out, drawing again"),
                None => {}
            }
            let Some(geometry) = self.space.output_geometry(&monitor.output) else {
                continue;
            };

            // Front to back: the pointer is above everything.
            let mut elements: Vec<OutputElements> = Vec::new();
            if geometry.to_f64().contains(pointer) {
                let position = pointer - geometry.loc.to_f64();
                match &self.cursor_status {
                    CursorImageStatus::Hidden => {}
                    // The application under the pointer supplies its own image.
                    CursorImageStatus::Surface(surface) => {
                        let hotspot = with_states(surface, |states| {
                            states
                                .data_map
                                .get::<Mutex<CursorImageAttributes>>()
                                .map(|attributes| attributes.lock().unwrap().hotspot)
                                .unwrap_or_default()
                        });
                        elements.extend(render_elements_from_surface_tree(
                            renderer,
                            surface,
                            (position - hotspot.to_f64()).to_physical(1.0).to_i32_round(),
                            1.0,
                            1.0,
                            Kind::Cursor,
                        ));
                    }
                    CursorImageStatus::Named(_) => {
                        if let Ok(arrow) = MemoryRenderBufferRenderElement::from_buffer(
                            renderer,
                            position.to_physical(1.0),
                            cursor,
                            None,
                            None,
                            None,
                            Kind::Cursor,
                        ) {
                            elements.push(OutputElements::Cursor(arrow));
                        }
                    }
                }
            }
            match space_render_elements(renderer, [&self.space], &monitor.output, 1.0) {
                Ok(space) => elements.extend(space.into_iter().map(OutputElements::Space)),
                Err(e) => tracing::error!("cannot collect windows to draw: {e:?}"),
            }

            let drawn = monitor
                .surface
                .render_frame(renderer, &elements, [0.0, 0.0, 0.0, 1.0], FrameFlags::empty())
                .map(|result| !result.is_empty);
            match drawn {
                Ok(true) => match monitor.surface.queue_frame(()) {
                    Ok(()) => monitor.pending_since = Some(Instant::now()),
                    Err(e) => tracing::error!("cannot present frame: {e}"),
                },
                Ok(false) => {}
                Err(e) => tracing::error!("cannot draw frame: {e}"),
            }
        }
    }
}
