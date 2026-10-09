//! Desktop policy: which window is shown where.
//!
//! The shell's frame fills the main display, and one application at a time covers the
//! workspace inside it. Every further display shows one application at full size, on
//! top of a backdrop window that the shell provides for it.

use smithay::{
    desktop::Window,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    input::pointer::MotionEvent,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::{compositor::with_states, seat::WaylandFocus, shell::xdg::XdgToplevelSurfaceData},
};

use crate::{EdexComp, focus::Focus};
use khadi_common::{
    Panel,
    ipc::{AppInfo, Command, DesktopState, Event, Place},
};

/// An application window and its place on the desktop.
pub struct App {
    pub window: Window,
    /// Index into `EdexComp::screens` of the display it lives on.
    pub screen: usize,
    /// Where it is on that display. At most one application is in each visible place.
    pub place: Place,
}

/// How a display's workspace is being used.
#[derive(Debug, Default, Clone, Copy)]
pub struct ScreenMode {
    /// Applications cover the whole display instead of the workspace inside the frame.
    pub fullscreen: bool,
    /// The right half of a split has the keyboard.
    pub side_focused: bool,
}

fn is_toplevel(window: &Window, surface: &WlSurface) -> bool {
    window.wl_surface().as_deref() == Some(surface)
}

/// What to give the keyboard to for it to reach `window`.
fn focus_of(window: &Window) -> Option<Focus> {
    match window.x11_surface() {
        Some(x11) => Some(Focus::X11(x11.clone())),
        None => window.wl_surface().map(|surface| Focus::Surface(surface.into_owned())),
    }
}

impl EdexComp {
    /// Tells the policy which displays exist, the main one first. Called by the backend
    /// at startup and whenever a display is added, removed or resized.
    pub fn set_screens(&mut self, screens: Vec<Rectangle<i32, Logical>>) {
        if screens.is_empty() {
            return;
        }
        // Applications on a display that went away come home, out of view.
        for app in &mut self.apps {
            if app.screen >= screens.len() {
                app.screen = 0;
                app.place = Place::Hidden;
            }
        }
        self.focus_screen = self.focus_screen.min(screens.len() - 1);
        if self.screens != screens {
            tracing::info!(?screens, "display layout");
        }
        let first = self.screens.is_empty();
        self.modes.resize(screens.len(), ScreenMode::default());
        self.screens = screens;
        self.arrange();

        if first {
            // Start with the pointer in the middle of the main display.
            let main = self.screens[0].to_f64();
            let location = main.loc + Point::from((main.size.w / 2.0, main.size.h / 2.0));
            let pointer = self.seat.get_pointer().unwrap();
            let under = self.surface_under(location);
            pointer.motion(
                self,
                under,
                &MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time: 0,
                },
            );
            pointer.frame(self);
        }
    }

    /// The display under `pos`, or the main one if `pos` is on none.
    pub fn screen_at(&self, pos: Point<f64, Logical>) -> usize {
        self.screens
            .iter()
            .position(|rect| rect.to_f64().contains(pos))
            .unwrap_or(0)
    }

    /// Where a pointer at `from` ends up when asked to move to `to`: it may cross onto
    /// another display, but never leave them all.
    pub fn clamp_to_screens(&self, from: Point<f64, Logical>, to: Point<f64, Logical>) -> Point<f64, Logical> {
        if self.screens.iter().any(|rect| rect.to_f64().contains(to)) {
            return to;
        }
        let Some(rect) = self.screens.get(self.screen_at(from)) else {
            return to;
        };
        let rect = rect.to_f64();
        Point::from((
            to.x.clamp(rect.loc.x, rect.loc.x + rect.size.w - 1.0),
            to.y.clamp(rect.loc.y, rect.loc.y + rect.size.h - 1.0),
        ))
    }

    /// Finds the shell, backdrop or application window whose toplevel is `surface`.
    ///
    /// Hidden applications are not in the `Space`, so lookups must go through here.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.shell
            .iter()
            .chain(self.backdrops.iter())
            .chain(self.apps.iter().map(|app| &app.window))
            .find(|window| is_toplevel(window, surface))
            .cloned()
    }

    /// The display a window is on.
    pub fn screen_of(&self, window: &Window) -> usize {
        if let Some(app) = self.apps.iter().find(|app| &app.window == window) {
            return app.screen;
        }
        self.backdrops
            .iter()
            .position(|backdrop| backdrop == window)
            .map_or(0, |index| index + 1)
    }

    /// The application in a slot of a display's workspace.
    fn in_place(&self, screen: usize, place: Place) -> Option<usize> {
        self.apps
            .iter()
            .position(|app| app.screen == screen && app.place == place)
    }

    fn mode(&self, screen: usize) -> ScreenMode {
        self.modes.get(screen).copied().unwrap_or_default()
    }

    /// The application that has the keyboard on a display: the one in the half last
    /// used, or the only one there is.
    fn focused_on(&self, screen: usize) -> Option<usize> {
        let side = self.in_place(screen, Place::Side);
        match side {
            Some(side) if self.mode(screen).side_focused => Some(side),
            _ => self.in_place(screen, Place::Main),
        }
    }

    /// Puts the application at `index` in a slot of its display, in place of whatever
    /// was there.
    fn place(&mut self, index: usize, place: Place) {
        let Some(screen) = self.apps.get(index).map(|app| app.screen) else {
            return;
        };
        for (i, app) in self.apps.iter_mut().enumerate() {
            if i == index {
                app.place = place;
            } else if app.screen == screen && app.place == place {
                app.place = Place::Hidden;
            }
        }
    }

    /// Brings the application at `index` into view and gives it the keyboard.
    fn activate(&mut self, index: usize) {
        let Some(app) = self.apps.get(index) else {
            return;
        };
        let screen = app.screen;
        // An application already in the right half stays there.
        let side = app.place == Place::Side;
        if !side {
            self.place(index, Place::Main);
        }
        if let Some(mode) = self.modes.get_mut(screen) {
            mode.side_focused = side;
        }
        self.focus_screen = screen;
    }

    /// The area applications occupy on a display: the workspace inside the frame on the
    /// main display, everything on the others, and everything anywhere in full screen.
    fn workspace_of(&self, screen: usize) -> Rectangle<i32, Logical> {
        let rect = self.screens.get(screen).or(self.screens.first()).copied().unwrap_or_default();
        // The terminal lives inside the frame, so full screen needs an application in
        // the main slot: beside the terminal, an application stays in the workspace.
        let covered = self.in_place(screen, Place::Main).is_some();
        if screen != 0 || (self.mode(screen).fullscreen && covered) {
            return rect;
        }
        to_rectangle(
            khadi_common::workspace(rect.size.w as f32, rect.size.h as f32, self.panels),
            rect.loc,
        )
    }

    /// Where each slot of a display's workspace is: the whole of it for the main slot
    /// alone, its halves when an application is in the side slot.
    fn slots(&self, screen: usize) -> (Rectangle<i32, Logical>, Rectangle<i32, Logical>) {
        let workspace = self.workspace_of(screen);
        if self.in_place(screen, Place::Side).is_none() {
            return (workspace, workspace);
        }
        let area = khadi_common::Rect {
            x: workspace.loc.x as f32,
            y: workspace.loc.y as f32,
            width: workspace.size.w as f32,
            height: workspace.size.h as f32,
        };
        let (left, right) = area.halves();
        (to_rectangle(left, (0, 0).into()), to_rectangle(right, (0, 0).into()))
    }

    /// Applies the layout to every window and gives the keyboard to the right one: the
    /// application in use on the display in use, or else the shell's terminal.
    pub fn arrange(&mut self) {
        let Some(primary) = self.screens.first().copied() else {
            return;
        };
        // A split needs a second application; without one the display is whole again.
        for (screen, mode) in self.modes.iter_mut().enumerate() {
            let split = self.apps.iter().any(|app| app.screen == screen && app.place == Place::Side);
            mode.side_focused &= split;
        }
        let focus_app = self.focused_on(self.focus_screen);

        if let Some(shell) = self.shell.clone() {
            configure(&shell, primary, true, focus_app.is_none());
            self.space.map_element(shell, primary.loc, false);
        }
        for (i, backdrop) in self.backdrops.clone().into_iter().enumerate() {
            match self.screens.get(i + 1).copied() {
                Some(rect) => {
                    configure(&backdrop, rect, true, false);
                    self.space.map_element(backdrop, rect.loc, false);
                }
                None => self.space.unmap_elem(&backdrop),
            }
        }
        for i in 0..self.apps.len() {
            let (window, screen, place) = {
                let app = &self.apps[i];
                (app.window.clone(), app.screen, app.place)
            };
            let (main, side) = self.slots(screen);
            let area = if place == Place::Side { side } else { main };
            // An application alone on a display in full screen is told so, which lets
            // it drop its own toolbars; in a half it is merely as large as it can be.
            let alone = self.mode(screen).fullscreen && main == side && area == self.screens.get(screen).copied().unwrap_or_default();
            configure(&window, area, alone, focus_app == Some(i));
            if place == Place::Hidden {
                self.space.unmap_elem(&window);
            } else {
                // Mapping also raises, which puts the application above what is behind it.
                self.space.map_element(window, area.loc, false);
            }
        }

        // X11 menus stay above the windows that were just raised.
        for popup in &self.x_popups {
            self.space.raise_element(popup, false);
        }

        let focus = focus_app
            .map(|i| &self.apps[i].window)
            .or(self.shell.as_ref())
            .and_then(focus_of);
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard.set_focus(self, focus, SERIAL_COUNTER.next_serial());
    }

    /// Registers a new toplevel: the shell's first window is the frame, its later ones
    /// are backdrops for further displays, and anything else is an application.
    pub fn add_window(&mut self, window: Window, from_shell: bool) {
        if from_shell && self.shell.is_none() {
            tracing::info!("shell window connected");
            self.shell = Some(window);
        } else if from_shell {
            tracing::info!(count = self.backdrops.len() + 1, "shell backdrop connected");
            self.backdrops.push(window);
        } else {
            // Applications open on the display the pointer is on, in the half in use.
            let pointer = self.seat.get_pointer().unwrap().current_location();
            let screen = self.screen_at(pointer);
            tracing::info!(screen, "application window opened");
            let side = self.in_place(screen, Place::Side).is_some() && self.mode(screen).side_focused;
            self.apps.push(App {
                window,
                screen,
                place: Place::Hidden,
            });
            self.place(self.apps.len() - 1, if side { Place::Side } else { Place::Main });
            self.focus_screen = screen;
        }
        self.arrange();
    }

    /// Forgets the window whose toplevel is `surface`.
    pub fn remove_window(&mut self, surface: &WlSurface) {
        if self.shell.as_ref().is_some_and(|w| is_toplevel(w, surface)) {
            // Whether this ends the session is decided when the shell's process exits.
            tracing::info!("shell window closed");
            if let Some(window) = self.shell.take() {
                self.space.unmap_elem(&window);
            }
        } else if let Some(index) = self.backdrops.iter().position(|w| is_toplevel(w, surface)) {
            let window = self.backdrops.remove(index);
            self.space.unmap_elem(&window);
        } else if let Some(index) = self.apps.iter().position(|app| is_toplevel(&app.window, surface)) {
            // What was behind the application comes back into view.
            let app = self.apps.remove(index);
            self.space.unmap_elem(&app.window);
            tracing::info!(remaining = self.apps.len(), "application window closed");
        }
        self.arrange();
    }

    /// Switches the slot in use to the next application on its display. The main slot
    /// passes through the terminal (or the backdrop) after the last one.
    pub fn cycle(&mut self) {
        let screen = self.focus_screen;
        let slot = match self.focused_on(screen) {
            Some(index) => self.apps[index].place,
            None => Place::Main,
        };
        // Applications in the other slot are not candidates.
        let here: Vec<usize> = (0..self.apps.len())
            .filter(|i| self.apps[*i].screen == screen && [slot, Place::Hidden].contains(&self.apps[*i].place))
            .collect();
        let current = here.iter().position(|i| self.apps[*i].place == slot);
        let next = match current {
            None => here.first().copied(),
            Some(position) => here.get(position + 1).copied(),
        };
        match next {
            Some(index) => self.place(index, slot),
            // Past the last one: the terminal for the main slot, the first again for
            // the side slot, which is never empty.
            None if slot == Place::Side => {
                if let Some(first) = here.first().copied() {
                    self.place(first, slot);
                }
            }
            None => {
                for app in self.apps.iter_mut().filter(|app| app.screen == screen && app.place == slot) {
                    app.place = Place::Hidden;
                }
            }
        }
        self.arrange();
    }

    /// Asks the current application to close. The terminal cannot be closed this way.
    pub fn close_active(&mut self) {
        if let Some(index) = self.focused_on(self.focus_screen) {
            self.close(index);
        }
    }

    fn close(&self, index: usize) {
        let Some(app) = self.apps.get(index) else {
            return;
        };
        if let Some(toplevel) = app.window.toplevel() {
            toplevel.send_close();
        } else if let Some(x11) = app.window.x11_surface() {
            let _ = x11.close();
        }
    }

    /// Moves the current application to the next display.
    pub fn move_active(&mut self) {
        if let Some(index) = self.focused_on(self.focus_screen) {
            self.move_app(index);
        }
    }

    fn move_app(&mut self, index: usize) {
        if self.screens.len() < 2 || index >= self.apps.len() {
            return;
        }
        self.apps[index].screen = (self.apps[index].screen + 1) % self.screens.len();
        self.apps[index].place = Place::Hidden;
        self.activate(index);
        self.arrange();
    }

    /// Splits the workspace in use between two windows, or makes it whole again.
    ///
    /// The window in view keeps the left half. The right half gets the application
    /// used before it; with a single application open, that one moves to the right
    /// and the terminal takes the left.
    pub fn toggle_split(&mut self) {
        let screen = self.focus_screen;
        let main = self.in_place(screen, Place::Main);
        if let Some(side) = self.in_place(screen, Place::Side) {
            // Whole again: the half in use wins the workspace.
            let keep_side = self.mode(screen).side_focused || main.is_none();
            self.apps[side].place = Place::Hidden;
            if keep_side {
                self.place(side, Place::Main);
            }
        } else {
            let other = (0..self.apps.len())
                .rev()
                .find(|i| self.apps[*i].screen == screen && self.apps[*i].place == Place::Hidden);
            match (other, main) {
                (Some(other), _) => self.apps[other].place = Place::Side,
                // Only the frame has a terminal to put beside a lone application.
                (None, Some(main)) if screen == 0 => self.apps[main].place = Place::Side,
                _ => return,
            }
            if let Some(mode) = self.modes.get_mut(screen) {
                mode.side_focused = true;
            }
        }
        self.arrange();
    }

    /// Gives the keyboard to the left or right half of a split workspace.
    pub fn focus_half(&mut self, right: bool) {
        let screen = self.focus_screen;
        if self.in_place(screen, Place::Side).is_some() {
            self.modes[screen].side_focused = right;
            self.arrange();
        }
    }

    /// Lets the applications on the display in use cover all of it, or stops that.
    pub fn toggle_fullscreen(&mut self) {
        let screen = self.focus_screen;
        self.set_fullscreen(screen, !self.mode(screen).fullscreen);
    }

    pub fn set_fullscreen(&mut self, screen: usize, fullscreen: bool) {
        if let Some(mode) = self.modes.get_mut(screen) {
            mode.fullscreen = fullscreen;
            self.arrange();
        }
    }

    /// Opens or collapses one of the frame's panels.
    pub fn toggle_panel(&mut self, panel: Panel) {
        self.panels.toggle(panel);
        self.arrange();
    }

    /// Uncovers the terminal and gives it the keyboard.
    fn show_terminal(&mut self) {
        for app in self.apps.iter_mut().filter(|app| app.screen == 0 && app.place == Place::Main) {
            app.place = Place::Hidden;
        }
        self.modes[0].side_focused = false;
        self.focus_screen = 0;
        self.arrange();
    }

    /// Asks the shell to show its application launcher.
    pub fn open_launcher(&mut self) {
        self.ask_shell(Event::LAUNCHER);
    }

    /// Asks the shell to show the settings.
    pub fn open_settings(&mut self) {
        self.ask_shell(Event::SETTINGS);
    }

    /// Uncovers the shell's workspace, where it shows such things, and sends `event`.
    fn ask_shell(&mut self, event: &str) {
        if self.modes.is_empty() {
            return;
        }
        self.show_terminal();
        if let Some(ipc) = self.ipc.as_mut() {
            ipc.send(event);
        }
    }

    /// A click landed at `pos`: that display, and that half of it, are now in use.
    pub fn focus_at(&mut self, pos: Point<f64, Logical>) {
        let screen = self.screen_at(pos);
        if screen >= self.modes.len() {
            return;
        }
        let split = self.in_place(screen, Place::Side).is_some();
        let (main, side) = self.slots(screen);
        let side_focused = if !split {
            false
        } else if side.to_f64().contains(pos) {
            true
        } else if main.to_f64().contains(pos) {
            false
        } else {
            // A click on the frame itself changes nothing.
            self.modes[screen].side_focused
        };
        if screen != self.focus_screen || side_focused != self.modes[screen].side_focused {
            self.focus_screen = screen;
            self.modes[screen].side_focused = side_focused;
            self.arrange();
        }
    }

    /// Carries out the shell's requests and tells it about any change in open applications.
    pub fn pump_ipc(&mut self) {
        let Some(mut ipc) = self.ipc.take() else {
            return;
        };
        for command in ipc.poll() {
            if self.modes.is_empty() {
                break;
            }
            match command {
                // The window may have closed while the request was on its way.
                Command::Activate(index) if index < self.apps.len() => {
                    self.activate(index);
                    self.arrange();
                }
                Command::Activate(_) => {}
                Command::Close(index) => self.close(index),
                Command::Move(index) => self.move_app(index),
                Command::Terminal => self.show_terminal(),
                Command::Split => self.toggle_split(),
                Command::Fullscreen => self.toggle_fullscreen(),
                Command::TogglePanel(panel) => self.toggle_panel(panel),
                Command::ApplyDisplays(choices) => self.apply_displays(choices),
                Command::KeepDisplays => self.keep_displays(),
                Command::RevertDisplays => self.revert_displays(),
                Command::ReloadInput => self.reload_input(),
            }
        }
        let last_screen = self.screens.len().saturating_sub(1);
        ipc.publish(&DesktopState {
            screens: self.screens.len().max(1),
            panels: self.panels,
            terminal_split: self.in_place(0, Place::Side).is_some() && self.in_place(0, Place::Main).is_none(),
            focus: self.focused_on(self.focus_screen),
            apps: self
                .apps
                .iter()
                .map(|app| AppInfo {
                    title: window_title(&app.window),
                    screen: app.screen.min(last_screen),
                    place: app.place,
                })
                .collect(),
        });
        ipc.publish_displays(&self.display_report());
        self.ipc = Some(ipc);
    }
}

fn to_rectangle(rect: khadi_common::Rect, origin: Point<i32, Logical>) -> Rectangle<i32, Logical> {
    Rectangle::new(
        (origin.x + rect.x.round() as i32, origin.y + rect.y.round() as i32).into(),
        (rect.width.round() as i32, rect.height.round() as i32).into(),
    )
}

/// The title a window gave itself, falling back to its application id.
fn window_title(window: &Window) -> String {
    if let Some(x11) = window.x11_surface() {
        let title = x11.title();
        return if title.is_empty() { x11.class() } else { title };
    }
    let Some(toplevel) = window.toplevel() else {
        return String::new();
    };
    with_states(toplevel.wl_surface(), |states| {
        let data = states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .unwrap()
            .lock()
            .unwrap();
        data.title
            .clone()
            .filter(|title| !title.is_empty())
            .or_else(|| data.app_id.clone())
            .unwrap_or_else(|| "untitled".to_string())
    })
}

/// Tells a window its place, whether it has a display to itself, and whether it has
/// the keyboard.
fn configure(window: &Window, area: Rectangle<i32, Logical>, fullscreen: bool, activated: bool) {
    if let Some(x11) = window.x11_surface() {
        if x11.is_fullscreen() != fullscreen {
            let _ = x11.set_fullscreen(fullscreen);
        }
        // X11 windows are told their position too: their menus are placed against it.
        if x11.geometry() != area {
            let _ = x11.configure(area);
        }
        if x11.is_maximized() == fullscreen {
            let _ = x11.set_maximized(!fullscreen);
        }
        if x11.is_activated() != activated {
            let _ = x11.set_activated(activated);
        }
        return;
    }
    let Some(toplevel) = window.toplevel() else {
        return;
    };
    toplevel.with_pending_state(|state| {
        state.size = Some(area.size);
        let (on, off) = if fullscreen {
            (xdg_toplevel::State::Fullscreen, xdg_toplevel::State::Maximized)
        } else {
            (xdg_toplevel::State::Maximized, xdg_toplevel::State::Fullscreen)
        };
        state.states.set(on);
        state.states.unset(off);
        if activated {
            state.states.set(xdg_toplevel::State::Activated);
        } else {
            state.states.unset(xdg_toplevel::State::Activated);
        }
    });
    // Before the first commit, the commit handler sends the initial configure instead.
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}
