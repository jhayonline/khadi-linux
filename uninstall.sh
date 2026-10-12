#!/bin/sh
# Removes everything install.sh put on the system.
set -eu
sudo rm -f /usr/local/bin/khadi /usr/local/bin/khadi-comp /usr/local/bin/khadi-music /usr/local/bin/khadi-session \
    /usr/share/wayland-sessions/khadi.desktop /usr/share/xdg-desktop-portal/khadi-portals.conf
sudo rm -f /etc/pam.d/khadi-lock
sudo rm -rf /usr/local/share/khadi /usr/local/lib/khadi
# What an installation under the old name, edex-rs, left behind.
sudo rm -f /usr/local/bin/edex-rs /usr/local/bin/edex-comp /usr/local/bin/edex-music /usr/local/bin/edex-rs-session \
    /usr/share/wayland-sessions/edex-rs.desktop /usr/share/xdg-desktop-portal/edex-rs-portals.conf
sudo rm -rf /usr/local/share/edex-rs /usr/local/lib/edex-rs
echo "Removed."
