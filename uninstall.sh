#!/bin/sh
# Removes everything install.sh put on the system.
set -eu
sudo rm -f /usr/local/bin/khadi /usr/local/bin/khadi-comp /usr/local/bin/khadi-session \
    /usr/share/wayland-sessions/khadi.desktop /usr/share/xdg-desktop-portal/khadi-portals.conf
sudo rm -rf /usr/local/share/khadi /usr/local/lib/khadi
echo "Removed."
