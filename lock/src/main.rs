//! khadi-lock — the Khadi lock screen.
//!
//! Locks the session through `ext-session-lock-v1`, covers every display, takes a
//! password and unlocks when PAM accepts it.
//!
//! The protocol is what makes this trustworthy rather than the program. The
//! compositor stops drawing the desktop and stops delivering input to it the moment
//! the lock is taken, and goes on doing that whatever becomes of this process: kill
//! it and the screen stays black and locked. So nothing here is security-critical
//! except the PAM call, and nothing here needs privileges.
//!
//! Run it with no arguments from inside a Khadi session.

mod auth;
mod draw;
mod theme;

use std::collections::HashMap;
use std::io::Write;
use std::os::fd::AsFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_keyboard::{self, WlKeyboard},
        wl_output::WlOutput,
        wl_registry::WlRegistry,
        wl_seat::{self, WlSeat},
        wl_shm::{self, WlShm},
        wl_shm_pool::WlShmPool,
        wl_surface::WlSurface,
    },
};
use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::ExtSessionLockManagerV1,
    ext_session_lock_surface_v1::{self, ExtSessionLockSurfaceV1},
    ext_session_lock_v1::{self, ExtSessionLockV1},
};

use draw::Mood;

/// How long a refusal is shown before the screen goes back to waiting.
const WRONG_SHOWN_FOR: Duration = Duration::from_millis(1500);

fn main() {
    let Some(user) = auth::current_user() else {
        eprintln!("khadi-lock: cannot tell who is logged in; refusing to lock");
        std::process::exit(1);
    };

    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(e) => {
            eprintln!("khadi-lock: no Wayland display: {e}");
            std::process::exit(1);
        }
    };
    let (globals, mut queue) = match registry_queue_init::<State>(&connection) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("khadi-lock: cannot talk to the compositor: {e}");
            std::process::exit(1);
        }
    };
    let qh = queue.handle();

    let compositor: WlCompositor = match globals.bind(&qh, 1..=6, ()) {
        Ok(global) => global,
        Err(e) => {
            eprintln!("khadi-lock: no wl_compositor: {e}");
            std::process::exit(1);
        }
    };
    let shm: WlShm = match globals.bind(&qh, 1..=1, ()) {
        Ok(global) => global,
        Err(e) => {
            eprintln!("khadi-lock: no wl_shm: {e}");
            std::process::exit(1);
        }
    };
    let manager: ExtSessionLockManagerV1 = match globals.bind(&qh, 1..=1, ()) {
        Ok(global) => global,
        Err(e) => {
            eprintln!("khadi-lock: this compositor does not support ext-session-lock-v1: {e}");
            std::process::exit(1);
        }
    };
    let _seat: WlSeat = match globals.bind(&qh, 1..=7, ()) {
        Ok(global) => global,
        Err(e) => {
            eprintln!("khadi-lock: no wl_seat: {e}");
            std::process::exit(1);
        }
    };

    // Every display is covered. One left out is a window onto the session.
    let outputs: Vec<WlOutput> = globals
        .contents()
        .clone_list()
        .into_iter()
        .filter(|global| global.interface == "wl_output")
        .map(|global| globals.registry().bind(global.name, global.version.min(4), &qh, ()))
        .collect();

    let lock = manager.lock(&qh, ());

    // How a finished PAM call gets the event loop's attention. See `wait` below.
    let (wake_read, wake_write) = match rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("khadi-lock: cannot make a pipe: {e}");
            std::process::exit(1);
        }
    };
    let wake_write = Arc::new(wake_write);

    let mut state = State {
        user,
        compositor,
        shm,
        outputs,
        lock: Some(lock),
        screens: HashMap::new(),
        theme: theme::Theme::load(),
        password: String::new(),
        mood: Mood::Typing(0),
        wrong_since: None,
        checker: auth::Checker::new(wake_write),
        keyboard: None,
        xkb: XkbState::new(),
        finished: false,
        unlocked: false,
    };

    while !state.unlocked && !state.finished {
        if let Err(e) = queue.dispatch_pending(&mut state) {
            eprintln!("khadi-lock: connection lost: {e}");
            // The compositor keeps the session locked; it does not need us alive.
            std::process::exit(1);
        }
        state.tick(&qh);
        if state.unlocked || state.finished {
            break;
        }
        if let Err(e) = connection.flush() {
            eprintln!("khadi-lock: connection lost: {e}");
            std::process::exit(1);
        }
        // `prepare_read` returns None when events arrived while we were working; go
        // round again and dispatch them rather than sleeping on an empty socket.
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let readable = wait(guard.connection_fd().as_fd(), wake_read.as_fd(), state.deadline());
        if readable.wayland
            && let Err(e) = guard.read()
        {
            eprintln!("khadi-lock: connection lost: {e}");
            std::process::exit(1);
        }
        if readable.wake {
            let mut drain = [0u8; 64];
            let _ = rustix::io::read(wake_read.as_fd(), &mut drain);
        }
    }

    if state.finished && !state.unlocked {
        // The compositor refused the lock, or took it away. It is not locked, and
        // saying so is better than exiting quietly as though it were.
        eprintln!("khadi-lock: the compositor did not lock the session");
        std::process::exit(1);
    }
    // Everything the compositor needs has been sent; make sure it leaves.
    let _ = connection.flush();
}

/// One display, and the buffers for it.
struct Screen {
    surface: WlSurface,
    /// Kept for the lifetime of the screen. Nothing reads it today — the configure
    /// is acked from the dispatch handler, which already has the object — but a
    /// screen that holds a surface it cannot name is harder to extend than one that
    /// does.
    #[allow(dead_code)]
    lock_surface: ExtSessionLockSurfaceV1,
    width: i32,
    height: i32,
    pool: Option<Pool>,
    /// What is on screen, so a repaint that changes nothing can be skipped.
    shown: Option<Mood>,
}

/// Two buffers in one mapping, used in turn so that a frame is never drawn into
/// memory the compositor is still reading.
struct Pool {
    pool: WlShmPool,
    map: memmap2::MmapMut,
    buffers: [WlBuffer; 2],
    /// Whether the compositor still holds each buffer.
    busy: [bool; 2],
    next: usize,
    width: i32,
    height: i32,
}

struct State {
    user: String,
    compositor: WlCompositor,
    shm: WlShm,
    outputs: Vec<WlOutput>,
    lock: Option<ExtSessionLockV1>,
    screens: HashMap<u32, Screen>,
    theme: theme::Theme,
    password: String,
    mood: Mood,
    wrong_since: Option<Instant>,
    checker: auth::Checker,
    keyboard: Option<WlKeyboard>,
    xkb: XkbState,
    finished: bool,
    unlocked: bool,
}

/// Which of the two things we wait on became readable.
struct Ready {
    wayland: bool,
    wake: bool,
}

/// Waits for the compositor to say something, for a PAM answer, or for the refusal
/// timer to run out — whichever comes first.
fn wait(wayland: std::os::fd::BorrowedFd<'_>, wake: std::os::fd::BorrowedFd<'_>, timeout: Option<Duration>) -> Ready {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut fds = [
        PollFd::new(&wayland, PollFlags::IN),
        PollFd::new(&wake, PollFlags::IN),
    ];
    let spec = timeout.map(|timeout| Timespec {
        tv_sec: timeout.as_secs() as _,
        tv_nsec: timeout.subsec_nanos() as _,
    });
    match poll(&mut fds, spec.as_ref()) {
        // A timeout wakes with nothing readable, which is the point: the caller goes
        // round the loop, `tick` sees the timer has expired and repaints.
        Ok(_) => Ready {
            wayland: !fds[0].revents().is_empty(),
            wake: !fds[1].revents().is_empty(),
        },
        Err(rustix::io::Errno::INTR) => Ready {
            wayland: false,
            wake: false,
        },
        Err(e) => {
            eprintln!("khadi-lock: cannot wait for events: {e}");
            std::process::exit(1);
        }
    }
}

impl State {
    /// How long until something needs doing with no event to prompt it.
    fn deadline(&self) -> Option<Duration> {
        let since = self.wrong_since?;
        Some(WRONG_SHOWN_FOR.saturating_sub(since.elapsed()))
    }

    /// Work that is not a reply to an event: the result of an attempt, and clearing
    /// a refusal once it has been shown long enough.
    fn tick(&mut self, qh: &QueueHandle<State>) {
        if let Some(outcome) = self.checker.poll() {
            match outcome {
                auth::Outcome::Accepted => {
                    if let Some(lock) = self.lock.take() {
                        lock.unlock_and_destroy();
                        self.unlocked = true;
                    }
                    return;
                }
                auth::Outcome::Rejected => {
                    self.password.clear();
                    self.mood = Mood::Wrong;
                    self.wrong_since = Some(Instant::now());
                }
            }
        }
        if let Some(since) = self.wrong_since
            && since.elapsed() >= WRONG_SHOWN_FOR
        {
            self.wrong_since = None;
            self.mood = Mood::Typing(0);
        }
        self.repaint(qh);
    }

    fn repaint(&mut self, qh: &QueueHandle<State>) {
        let theme = self.theme;
        let mood = self.mood;
        for screen in self.screens.values_mut() {
            screen.paint(&self.shm, qh, &theme, mood);
        }
    }

    /// A key that is not a character: Enter, Backspace and the ways of clearing.
    fn special(&mut self, keysym: u32) -> bool {
        const RETURN: u32 = 0xFF0D;
        const KP_ENTER: u32 = 0xFF8D;
        const BACKSPACE: u32 = 0xFF08;
        const ESCAPE: u32 = 0xFF1B;
        match keysym {
            RETURN | KP_ENTER => {
                if !self.checker.busy() && !self.password.is_empty() {
                    self.mood = Mood::Checking;
                    self.wrong_since = None;
                    self.checker
                        .submit(self.user.clone(), std::mem::take(&mut self.password));
                }
                true
            }
            BACKSPACE => {
                self.password.pop();
                self.after_edit();
                true
            }
            ESCAPE => {
                self.password.clear();
                self.after_edit();
                true
            }
            _ => false,
        }
    }

    fn after_edit(&mut self) {
        if !self.checker.busy() {
            self.wrong_since = None;
            self.mood = Mood::Typing(self.password.chars().count());
        }
    }
}

impl Screen {
    fn paint(&mut self, shm: &WlShm, qh: &QueueHandle<State>, theme: &theme::Theme, mood: Mood) {
        if self.width <= 0 || self.height <= 0 {
            return;
        }
        // A size change always repaints; otherwise only a change of mood does.
        let stale = self
            .pool
            .as_ref()
            .is_none_or(|pool| pool.width != self.width || pool.height != self.height);
        if !stale && self.shown == Some(mood) {
            return;
        }
        if stale {
            self.pool = Pool::new(shm, qh, self.width, self.height);
        }
        let Some(pool) = self.pool.as_mut() else {
            return;
        };
        let Some(index) = pool.free_slot() else {
            // Both buffers are still with the compositor. The next event repaints.
            return;
        };

        let count = (pool.width * pool.height) as usize;
        let offset = index * count * 4;
        let bytes = &mut pool.map[offset..offset + count * 4];
        // Safety: the mapping is page-aligned and `u32` has no invalid values, so a
        // run of 4-byte pixels can be addressed as u32 at a 4-byte-aligned offset.
        let (prefix, pixels, suffix) = unsafe { bytes.align_to_mut::<u32>() };
        if !prefix.is_empty() || !suffix.is_empty() {
            return;
        }
        draw::render(pixels, pool.width, pool.height, theme, mood);

        pool.busy[index] = true;
        pool.next = 1 - index;
        self.surface.attach(Some(&pool.buffers[index]), 0, 0);
        self.surface.damage_buffer(0, 0, pool.width, pool.height);
        self.surface.commit();
        self.shown = Some(mood);
    }
}

impl Pool {
    fn new(shm: &WlShm, qh: &QueueHandle<State>, width: i32, height: i32) -> Option<Pool> {
        let stride = width * 4;
        let one = (stride * height) as usize;
        let total = one * 2;

        let file = memfile(total)?;
        let map = unsafe { memmap2::MmapMut::map_mut(&file) }.ok()?;
        let pool = shm.create_pool(file.as_fd(), total as i32, qh, ());
        let buffers = [
            pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, 0usize),
            pool.create_buffer(one as i32, width, height, stride, wl_shm::Format::Argb8888, qh, 1usize),
        ];
        Some(Pool {
            pool,
            map,
            buffers,
            busy: [false, false],
            next: 0,
            width,
            height,
        })
    }

    fn free_slot(&self) -> Option<usize> {
        if !self.busy[self.next] {
            Some(self.next)
        } else if !self.busy[1 - self.next] {
            Some(1 - self.next)
        } else {
            None
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for buffer in &self.buffers {
            buffer.destroy();
        }
        self.pool.destroy();
    }
}

/// An anonymous file to share with the compositor.
fn memfile(size: usize) -> Option<std::fs::File> {
    use rustix::fs::{MemfdFlags, memfd_create};
    let fd = memfd_create("khadi-lock", MemfdFlags::CLOEXEC).ok()?;
    let mut file = std::fs::File::from(fd);
    // Written rather than truncated: on a filesystem that cannot allocate the space,
    // ftruncate succeeds and the process is killed with SIGBUS on first touch.
    file.write_all(&vec![0u8; size]).ok()?;
    file.flush().ok()?;
    Some(file)
}

// --- keyboard ---------------------------------------------------------------

/// The xkb keymap and the state that tracks held modifiers.
struct XkbState {
    context: xkbcommon::xkb::Context,
    state: Option<xkbcommon::xkb::State>,
}

impl XkbState {
    fn new() -> XkbState {
        XkbState {
            context: xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS),
            state: None,
        }
    }
}

// --- dispatch ---------------------------------------------------------------

impl Dispatch<ExtSessionLockV1, ()> for State {
    fn event(
        state: &mut Self,
        _lock: &ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _data: &(),
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            // The desktop is off the screen. Only now is it safe to draw.
            ext_session_lock_v1::Event::Locked => {
                let outputs = state.outputs.clone();
                for (index, output) in outputs.into_iter().enumerate() {
                    let Some(lock) = state.lock.as_ref() else {
                        break;
                    };
                    let surface = state.compositor.create_surface(qh, ());
                    let lock_surface =
                        lock.get_lock_surface(&surface, &output, qh, index as u32);
                    state.screens.insert(
                        index as u32,
                        Screen {
                            surface,
                            lock_surface,
                            width: 0,
                            height: 0,
                            pool: None,
                            shown: None,
                        },
                    );
                }
            }
            // Either the compositor refused to lock, or the lock ended elsewhere.
            ext_session_lock_v1::Event::Finished => {
                state.finished = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtSessionLockSurfaceV1, u32> for State {
    fn event(
        state: &mut Self,
        surface: &ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        id: &u32,
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure { serial, width, height } = event {
            surface.ack_configure(serial);
            let theme = state.theme;
            let mood = state.mood;
            if let Some(screen) = state.screens.get_mut(id) {
                screen.width = width as i32;
                screen.height = height as i32;
                screen.paint(&state.shm, qh, &theme, mood);
            }
        }
    }
}

impl Dispatch<WlBuffer, usize> for State {
    fn event(
        state: &mut Self,
        buffer: &WlBuffer,
        event: wayland_client::protocol::wl_buffer::Event,
        index: &usize,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if matches!(event, wayland_client::protocol::wl_buffer::Event::Release) {
            for screen in state.screens.values_mut() {
                if let Some(pool) = screen.pool.as_mut()
                    && pool.buffers[*index] == *buffer
                {
                    pool.busy[*index] = false;
                }
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _connection: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: wayland_client::WEnum::Value(capabilities),
        } = event
            && capabilities.contains(wl_seat::Capability::Keyboard)
            && state.keyboard.is_none()
        {
            state.keyboard = Some(seat.get_keyboard(qh, ()));
        }
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _keyboard: &WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap {
                format: wayland_client::WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                // Safety: the compositor promises a readable mapping of `size` bytes
                // holding a NUL-terminated keymap.
                let keymap = unsafe {
                    xkbcommon::xkb::Keymap::new_from_fd(
                        &state.xkb.context,
                        fd,
                        size as usize,
                        xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1,
                        xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
                    )
                };
                match keymap {
                    Ok(Some(keymap)) => {
                        state.xkb.state = Some(xkbcommon::xkb::State::new(&keymap));
                    }
                    _ => eprintln!("khadi-lock: cannot read the keyboard layout"),
                }
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(xkb) = state.xkb.state.as_mut() {
                    xkb.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                }
            }
            wl_keyboard::Event::Key {
                key,
                state: wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed),
                ..
            } => {
                // Wayland reports evdev codes; xkb counts from 8 higher.
                let code = key + 8;
                let Some((keysym, typed)) = state.xkb.state.as_ref().map(|xkb| {
                    (xkb.key_get_one_sym(code.into()).raw(), xkb.key_get_utf8(code.into()))
                }) else {
                    return;
                };
                if state.special(keysym) {
                    return;
                }
                if state.checker.busy() {
                    return;
                }
                // Control characters are not part of a password; letting them in
                // would mean Ctrl+C silently became two characters nobody typed.
                if !typed.is_empty() && !typed.chars().any(|c| c.is_control()) {
                    state.password.push_str(&typed);
                    state.after_edit();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _state: &mut Self,
        _registry: &WlRegistry,
        _event: wayland_client::protocol::wl_registry::Event,
        _data: &GlobalListContents,
        _connection: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Displays that appear while locked are not covered. See the note in README.
    }
}

macro_rules! ignore {
    ($($interface:ty),* $(,)?) => {$(
        impl Dispatch<$interface, ()> for State {
            fn event(
                _state: &mut Self,
                _proxy: &$interface,
                _event: <$interface as wayland_client::Proxy>::Event,
                _data: &(),
                _connection: &Connection,
                _qh: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}

ignore!(
    WlCompositor,
    WlShm,
    WlShmPool,
    WlSurface,
    WlOutput,
    ExtSessionLockManagerV1,
);
