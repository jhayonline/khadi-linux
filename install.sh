#!/bin/sh
# Builds edex-rs and registers it as a session on the login screen.
set -eu
cd "$(dirname "$0")"

if ! command -v cage >/dev/null; then
    echo "Note: cage is not installed, so the session has no fallback if edex-comp" >&2
    echo "cannot drive the display. To add one: sudo apt install cage" >&2
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
