#!/bin/sh
# Removes everything install.sh put on the system.
set -eu
sudo rm -f /usr/local/bin/des-ui /usr/local/bin/des-comp /usr/local/bin/des-ui-session \
    /usr/local/bin/des-ui-lock /etc/pam.d/des-ui-lock \
    /usr/share/wayland-sessions/des-ui.desktop /usr/share/xdg-desktop-portal/des-ui-portals.conf
sudo rm -rf /usr/local/share/des-ui /usr/local/lib/des-ui
echo "Removed."
