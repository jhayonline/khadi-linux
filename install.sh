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
# The first build under this name falls back to the last one under the old name.
if [ ! -x /usr/local/bin/khadi ] && [ -x /usr/local/bin/edex-rs ] && [ -x /usr/local/bin/edex-comp ]; then
    sudo install -Dm755 /usr/local/bin/edex-rs /usr/local/lib/khadi/previous/khadi
    sudo install -Dm755 /usr/local/bin/edex-comp /usr/local/lib/khadi/previous/khadi-comp
fi
sudo install -Dm755 target/release/khadi /usr/local/bin/khadi
sudo install -Dm755 target/release/khadi-comp /usr/local/bin/khadi-comp
sudo install -Dm755 target/release/khadi-music /usr/local/bin/khadi-music
# Without this file PAM has no rules for the lock screen and refuses every
# password, so the screen locks and nothing can unlock it. Never overwritten:
# a machine whose administrator changed the unlock rules keeps them.
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

# First-boot setup. Installed, deliberately not enabled: the unit only runs when
# /var/lib/khadi/provisioning/pending exists, and an ordinary machine — one that
# already has an account on it — must never be asked who it belongs to. An ISO
# install that defers provisioning writes the marker and enables the unit.
sudo install -Dm755 provisioning/khadi-provision-owner /usr/local/bin/khadi-provision-owner
sudo install -Dm644 provisioning/setup-form.sh /usr/local/share/khadi/provisioning/setup-form.sh
sudo install -Dm644 provisioning/khadi-provision-owner.service \
    /etc/systemd/system/khadi-provision-owner.service


# This desktop was called edex-rs before. Whatever still starts a session under that
# name gets this one; the rest of the old installation goes once nothing of it runs.
if [ -e /usr/local/bin/edex-rs-session ] || [ -L /usr/local/bin/edex-rs-session ]; then
    sudo ln -sfn khadi-session /usr/local/bin/edex-rs-session
    sudo rm -f /usr/share/wayland-sessions/edex-rs.desktop
fi
if [ -e /usr/local/bin/edex-comp ]; then
    if [ -f /usr/local/bin/edex-rs-greeter ] && [ ! -L /usr/local/bin/edex-rs-greeter ]; then
        echo "The login screen still runs under the old name, edex-rs. Run ./install-login.sh"
        echo "to bring it over; the old programs are left in place until then."
    elif pgrep -x edex-comp >/dev/null; then
        echo "The session running now is the one installed under the old name, edex-rs. It is"
        echo "left in place; run ./install.sh once more after logging in again to clear it away."
    else
        sudo rm -f /usr/local/bin/edex-rs /usr/local/bin/edex-comp /usr/local/bin/edex-music \
            /usr/local/bin/edex-rs-session /usr/share/xdg-desktop-portal/edex-rs-portals.conf
        sudo rm -rf /usr/local/share/edex-rs /usr/local/lib/edex-rs
    fi
fi

if [ "${XDG_CURRENT_DESKTOP:-}" = khadi ]; then
    echo "Installed. Super+Shift+R restarts the shell on the new build (terminal tabs close);"
    echo "a new compositor takes effect the next time you log in."
else
    echo "Installed. Log out and pick 'Khadi' from the session menu on the login screen."
fi
