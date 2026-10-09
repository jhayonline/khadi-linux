#!/bin/sh
# Builds edex-rs and registers it as a session on the login screen.
set -eu
cd "$(dirname "$0")"

# Arch does not split development headers into their own packages, so what the
# build needs and what the session needs are the same names. Checked by the
# library smithay links against rather than by package name, because a missing
# one otherwise surfaces as a linker error about a symbol nobody recognises.
missing=
for pc in libinput libseat gbm libudev wayland-server xkbcommon; do
    pkg-config --exists "$pc" 2>/dev/null || missing="$missing $pc"
done
if [ -n "$missing" ]; then
    echo "Missing build dependencies:$missing" >&2
    echo "  sudo pacman -S --needed libinput seatd mesa systemd-libs wayland libxkbcommon" >&2
    exit 1
fi

if ! command -v cage >/dev/null; then
    echo "Note: cage is not installed, so the session has no fallback if edex-comp" >&2
    echo "cannot drive the display. To add one: sudo pacman -S cage" >&2
fi

if ! command -v Xwayland >/dev/null; then
    echo "Note: Xwayland is not installed, so X11 applications will not run." >&2
    echo "To add it: sudo pacman -S xorg-xwayland" >&2
fi

cargo build --release

# Keep the build being replaced: the session falls back to it if the new one cannot start.
for program in edex-rs edex-comp; do
    if [ -x "/usr/local/bin/$program" ]; then
        sudo install -Dm755 "/usr/local/bin/$program" "/usr/local/lib/edex-rs/previous/$program"
    fi
done
sudo install -Dm755 target/release/edex-rs /usr/local/bin/edex-rs
sudo install -Dm755 target/release/edex-comp /usr/local/bin/edex-comp
sudo install -Dm755 session/edex-rs-session /usr/local/bin/edex-rs-session
sudo install -Dm644 session/edex-rs.desktop /usr/share/wayland-sessions/edex-rs.desktop
sudo install -Dm644 session/edex-rs-portals.conf /usr/share/xdg-desktop-portal/edex-rs-portals.conf
sudo install -d /usr/local/share/edex-rs/themes
sudo install -m644 themes/*.json /usr/local/share/edex-rs/themes/

if [ "${XDG_CURRENT_DESKTOP:-}" = edex-rs ]; then
    echo "Installed. Super+Shift+R restarts the shell on the new build (terminal tabs close);"
    echo "a new compositor takes effect the next time you log in."
else
    echo "Installed. Log out and pick 'DEs-UI' from the session menu on the login screen."
fi
