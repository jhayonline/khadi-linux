# Khadi

A keyboard-driven, terminal-first Arch desktop, written in Rust: a Wayland
compositor and a shell that run as their own login session.

Khadi is [DEs-UI](https://github.com/coxy-trophy/des-ui) by Coxwell Wussah,
renamed and made to build and install on Arch. That project began as a rewrite
of [eDEX-UI](https://github.com/GitSquared/edex-ui), and almost everything good
here is his: the compositor, the shell, the settings, the launcher, the globe.
Khadi's own history before this is a different desktop — Hyprland, with the
shell in a webview — and is at the tag `pre-des-ui`.

It has two programs:

- **khadi** (`shell/`) — the interface: terminal, system panels, file browser.
- **khadi-comp** (`compositor/`) — a Wayland compositor that shows the shell
  fullscreen and places other applications in the middle of its frame.
- **khadi-lock** (`lock/`) — the lock screen.

## Try it in a window

    cargo build
    ./target/debug/khadi-comp                      # the whole desktop
    ./target/debug/khadi-comp -- --theme blade     # arguments after -- go to the shell
    ./target/debug/khadi --windowed             # the shell on its own

Open applications with the launcher, or by starting them from the terminal. Hold
Super (or Ctrl+Alt, which works when running in a window):

- **Space** — open the launcher: type to search, Enter to open, Esc to cancel
- **,** — open the settings
- **Tab** — switch between the terminal and the applications on this display
- **S** — split the workspace between two windows, or make it whole again
- **Left / Right** — give the keyboard to that half of a split
- **F** — full screen: the applications on this display cover all of it
- **[  ]  \\** — collapse or open the left, right and bottom panels
- **Q** — close the current application
- **O** — move the current application to the next display
- **Esc** — lock the screen

The bar above the workspace lists the terminal tabs and every open application;
click one to switch to it. The application in use shows an arrow, to send it to
the next display, and a cross, to close it. `SPLIT`, `FULL` and `APPS` do what the
keys do, and the three small switches in the status strip collapse the panels.

A split keeps the window in view on the left and puts the application used before
it on the right. With one application open, that one goes right and the terminal
stays on the left.

With several displays, modes and arrangement follow what you chose in the settings,
or else the layout saved in GNOME's display settings (`~/.config/monitors.xml`)
when it covers the connected monitors.
Otherwise the frame is on a laptop's own panel and the others extend to its right.
Each further display shows its applications at full size, split or not.
Applications open on the display the pointer is on.

`khadi --list-apps` prints what the launcher offers.

The globe marks this machine and the places it has connections open to. Positions
come from looking addresses up at get.geojs.io, which tells that service the public
addresses this machine talks to; each is asked about once and kept in
`~/.cache/khadi/places.tsv`. Your own position is guessed from your address
unless you set it.

## Locking the screen

**Super+Esc** locks it. Type the password and press Enter; Esc clears what was
typed. A wrong password is shown for a moment and then the screen goes back to
waiting.

The compositor does the locking, through `ext-session-lock-v1`, and it is the
compositor that makes it mean something: from the moment the lock is taken it stops
drawing the desktop and stops delivering input to it, and goes on doing that
whatever becomes of `khadi-lock`. Kill the lock screen and the display stays black
and locked rather than falling open. Any other client speaking that protocol works
too — `swaylock`, for instance.

The cost of that guarantee: if `khadi-lock` crashes, the screen is black and there
is no way back into the session. **Ctrl+Alt+F2** reaches a virtual terminal, where
ending `khadi-comp` gets the machine back and loses the session. Sway recovers by
letting a second lock client take the screen over; smithay 0.7 does not allow that,
for the reason written down at the top of `compositor/src/lock.rs`.

Not yet: nothing locks the screen on its own. There is no idle timer and no lock on
suspend, so this is a lock you have to ask for.

## Settings

**Super+,** or `SETTINGS` in the status strip opens them in place of the terminal;
Esc closes. Changes apply at once and are saved.

- **Appearance** — theme
- **Terminal** — font, text size, line height, cursor shape and blink, bold in
  bright colours, copy on select, scrollback
- **Frame** — which panels are open, now and at every start
- **Displays** — each monitor's resolution and refresh rate, their order left to
  right, and which one is the main display
- **Network** — Wi-Fi on or off, and joining, leaving and forgetting networks
- **Bluetooth** — on or off, and pairing, connecting and forgetting devices
- **Keyboard** — layout, and how held keys repeat
- **Mouse & touchpad** — natural scrolling, tap to click, pointer speed, left-handed
- **Globe** — rotation, online lookups, and your location
- **Sound & screen** — volume and brightness
- **Session** — where the logs are, and logging out

A display change is tried for 15 seconds and kept only if you confirm it; otherwise
it goes back by itself, so a mode your monitor cannot show does not strand you.
Confirmed layouts are saved in `~/.config/khadi/displays`, one per set of
monitors, and come before GNOME's saved layout.

The rest are kept in `~/.config/khadi/config` as `key = value` lines, which can also
be edited by hand. The compositor reads the keyboard, mouse and touchpad
settings from the same file. Network and Bluetooth go through `nmcli` and
`bluetoothctl`; a Wi-Fi password is handed to `nmcli` on its command line. Two can be set from a terminal:

    khadi --set-location 48.8566,2.3522,Paris   # or: --set-location auto
    khadi --geo off                             # no lookups at all; on to resume

X11 applications run through Xwayland, which `khadi-comp` starts and manages; the
clipboard is shared between them and Wayland applications.

The look is called Signal (`shell/src/ui.rs`): fixed greys, two typefaces, and one
accent colour. A theme sets that accent and the terminal's colours, in the JSON
format eDEX-UI uses, so its theme files work here. `themes/` holds the built-in
one; your own go in `~/.config/khadi/themes/`.

## Install as a login session

    sudo pacman -S --needed $(sed 's/#.*//' packages.txt | tr -s '[:space:]' ' ')
    ./install.sh

`packages.txt` lists every package with a line saying what needs it. Arch does
not split headers into their own packages, so the build dependencies and the
runtime ones are mostly the same names; `install.sh` checks for the libraries
smithay links against before it starts, because a missing one otherwise shows
up as a linker error naming a symbol rather than a package.

Arch ships no display manager, so nothing will offer the session until one is
installed. See the note at the end of `packages.txt`.

Then log out and choose **Khadi** from the session menu on the login screen.
Exiting the last shell, or Ctrl+Shift+Q, ends the session. `./uninstall.sh`
removes it again.

In the session, `khadi-comp` drives the displays and input devices itself (one GPU,
every monitor connected to it, including ones plugged in later). If that fails in the first seconds it falls back to running inside
`cage`, and the reason is in `~/.local/state/khadi/session.log`. What applications started
from the launcher print goes to `apps.log` next to it.

Ways out if something goes wrong: **Ctrl+Alt+Backspace** ends the session, and
**Ctrl+Alt+F1…F12** switch virtual terminals.

## Working inside it while changing it

- `./dev.sh` builds the working copy and runs it in a window inside the running
  session. Nothing installed is touched.
- `./install.sh` can be run from inside the session. **Super+Shift+R** then restarts
  the shell on the new build: its terminal tabs close, applications stay open. A new
  compositor takes effect at the next login.
- If the shell crashes it is started again and applications stay open. Only quitting
  it (Ctrl+Shift+Q, or closing the last tab) ends the session.
- `install.sh` keeps the build it replaces. If a new build cannot start at login, the
  session tries that one, then `cage`.
- Anything that must survive a shell restart should not run in the shell's own
  terminal tabs: use a terminal application such as `ptyxis`, or `tmux`.

## Layout

- `shell/src/term.rs` — shell on a PTY, VT parsing, grid rendering, key encoding
- `shell/src/sysmon.rs` — CPU, memory, process and network sampling
- `shell/src/ui.rs` — the interface language: colours, typefaces, shared parts
- `shell/src/panels.rs` — side columns, file browser, status strip
- `shell/src/globe.rs`, `geo.rs` — the rotating globe and the places on it
- `shell/src/settings.rs`, `config.rs` — the settings screen and the file behind it
- `shell/src/radio.rs` — Wi-Fi and Bluetooth
- `common/src/input.rs` — keyboard, mouse and touchpad settings
- `shell/src/apps.rs`, `launcher.rs` — installed applications and the launcher
- `shell/src/theme.rs` — eDEX-UI theme loader
- `compositor/src/policy.rs` — which window is shown on which display
- `compositor/src/udev.rs`, `winit.rs` — the hardware and windowed backends
- `compositor/src/displays.rs`, `monitors.rs` — display settings and saved layouts
- `compositor/src/xwayland.rs`, `focus.rs` — X11 applications
- `compositor/src/lock.rs` — the session lock, and what it refuses to do
- `lock/src/main.rs` — the lock screen: Wayland by hand, no toolkit
- `lock/src/auth.rs` — PAM, on a thread of its own
- `compositor/src/ipc.rs`, `shell/src/desktop.rs` — the socket the two talk over
- `common/` — frame geometry and that socket's protocol, shared by both
- `session/` — login session entry and launcher

## Not done yet

- Locking on idle, and on suspend; a display plugged in while locked is not covered
- Mirroring displays
- Keyboard layout setting; display scaling for high-density screens
- Dialogs at their natural size; sharing the middle-click selection with X11 applications
- Turning a display off; placing displays above one another; scaling
- Wired and VPN connections in the settings; Bluetooth devices that need a code
- A login screen of its own (designed, not built: it still uses the system's)
- From the original: on-screen keyboard, sound effects, mouse reporting to
  terminal apps

## Licence

MIT; see `LICENSE`, which is DEs-UI's and keeps Coxwell Wussah's copyright
notice, as the licence requires. `compositor/` is derived from Smithay's
`smallvil` example, also MIT. The bundled typefaces, Chakra Petch and JetBrains Mono (`shell/assets/fonts/`),
are under the SIL Open Font License.

eDEX-UI's own theme files are GPL-3.0 and are not part of this repository. They
work as themes here: copy them into `~/.config/khadi/themes/`.
