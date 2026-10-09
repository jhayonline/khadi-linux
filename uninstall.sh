#!/bin/sh
# Removes everything install.sh put on the system.
set -eu
sudo rm -f /usr/local/bin/edex-rs /usr/local/bin/edex-comp /usr/local/bin/edex-rs-session \
    /usr/share/wayland-sessions/edex-rs.desktop /usr/share/xdg-desktop-portal/edex-rs-portals.conf
sudo rm -rf /usr/local/share/edex-rs /usr/local/lib/edex-rs
echo "Removed."
