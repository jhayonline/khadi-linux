#!/bin/sh
# Puts GDM back as this machine's login screen. Takes effect at the next boot.
set -eu

sudo systemctl disable greetd 2>/dev/null || true
sudo systemctl enable --force gdm3 2>/dev/null || sudo systemctl enable --force gdm

config=/etc/greetd/config.toml
if [ -f "$config.before-des-ui" ]; then
    sudo mv "$config.before-des-ui" "$config"
fi
sudo rm -f /usr/local/bin/des-ui-greeter /usr/local/bin/edex-rs-greeter
if [ -L /var/lib/edex-rs ]; then
    sudo rm -f /var/lib/edex-rs
fi
echo "GDM is the login screen again from the next boot. greetd itself is still installed;"
echo "remove it with: sudo apt remove greetd"
