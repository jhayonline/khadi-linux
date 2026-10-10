#!/bin/sh
# Builds khadi and registers it as a session on the login screen.
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
    echo "Note: cage is not installed, so the session has no fallback if khadi-comp" >&2
    echo "cannot drive the display. To add one: sudo pacman -S cage" >&2
fi

if ! command -v Xwayland >/dev/null; then
    echo "Note: Xwayland is not installed, so X11 applications will not run." >&2
    echo "To add it: sudo pacman -S xorg-xwayland" >&2
fi

cargo build --release

# Keep the build being replaced: the session falls back to it if the new one cannot start.
for program in khadi khadi-comp; do
    if [ -x "/usr/local/bin/$program" ]; then
        sudo install -Dm755 "/usr/local/bin/$program" "/usr/local/lib/khadi/previous/$program"
    fi
done
sudo install -Dm755 target/release/khadi /usr/local/bin/khadi
sudo install -Dm755 target/release/khadi-comp /usr/local/bin/khadi-comp
sudo install -Dm755 target/release/khadi-lock /usr/local/bin/khadi-lock
# Without this file PAM has no rules for khadi-lock and refuses every password, so
# the screen locks and nothing can unlock it. Installed before the binary is ever
# run, and never overwritten: a machine whose administrator has changed the unlock
# rules should keep them across an upgrade.
if [ ! -e /etc/pam.d/khadi-lock ]; then
    sudo install -Dm644 session/khadi-lock.pam /etc/pam.d/khadi-lock
fi
sudo install -Dm755 session/khadi-session /usr/local/bin/khadi-session
sudo install -Dm644 session/khadi.desktop /usr/share/wayland-sessions/khadi.desktop
sudo install -Dm644 session/khadi-portals.conf /usr/share/xdg-desktop-portal/khadi-portals.conf
sudo install -d /usr/local/share/khadi/themes
sudo install -m644 themes/*.json /usr/local/share/khadi/themes/
# The name Khadi goes by. Not /etc/os-release — see the top of the file for why.
sudo install -Dm644 session/os-release /usr/local/share/khadi/os-release

if [ "${XDG_CURRENT_DESKTOP:-}" = khadi ]; then
    echo "Installed. Super+Shift+R restarts the shell on the new build (terminal tabs close);"
    echo "a new compositor takes effect the next time you log in."
else
    echo "Installed. Log out and pick 'Khadi' from the session menu on the login screen."
fi
