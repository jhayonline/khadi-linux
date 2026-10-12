#!/bin/sh
# Puts the Khadi splash on the screen while this machine boots and shuts down, in
# place of the maker's logo. It is drawn by Plymouth; this adds a theme for it and
# makes it the one in use.
#
# The pictures are made in the colours of the theme you use now; run this again after
# changing theme. To go back to what was there before:  ./uninstall-boot-splash.sh
#
# This rebuilds the boot image (the "initramfs"), which takes a minute.
#
# Arch, not Debian: upstream drives this with update-alternatives and
# update-initramfs, neither of which exists here. The Arch equivalents are
# plymouth-set-default-theme and mkinitcpio, and Arch additionally needs the
# `plymouth` hook named in /etc/mkinitcpio.conf — without it the theme is installed,
# the initramfs is rebuilt, and nothing appears at boot.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
theme=/usr/share/plymouth/themes/khadi
conf=/etc/mkinitcpio.conf

if ! command -v plymouth-set-default-theme >/dev/null; then
    echo "Plymouth is not installed. Add it with:  sudo pacman -S plymouth" >&2
    exit 1
fi
# Arch ships every Plymouth plugin in the one package, so a missing script.so
# means a broken install rather than a package to add.
if ! find /usr/lib -name script.so -path '*plymouth*' 2>/dev/null | grep -q .; then
    echo "Plymouth's script plugin is missing; this theme cannot be drawn." >&2
    echo "Try:  sudo pacman -S --needed plymouth" >&2
    exit 1
fi

shell=/usr/local/bin/khadi
[ -x "$shell" ] || shell="$here/target/release/khadi"
if ! [ -x "$shell" ]; then
    echo "khadi is not installed or built yet: run ./install.sh first." >&2
    exit 1
fi

pictures=$(mktemp -d)
trap 'rm -rf "$pictures"' EXIT
"$shell" --write-boot-splash "$pictures"

sudo install -d "$theme"
sudo install -m 644 "$pictures"/*.png "$here/session/boot/khadi.plymouth" "$here/session/boot/khadi.script" "$theme/"
sudo plymouth-set-default-theme khadi

# The hook, added once and only if it is missing. The original is kept so that
# uninstall-boot-splash.sh can put it back exactly, rather than guessing at an edit
# to undo.
if ! grep -qE '^HOOKS=.*\bplymouth\b' "$conf"; then
    echo "Adding the plymouth hook to $conf"
    sudo cp -n "$conf" "$conf.khadi-backup"
    # After `udev` on a plain install, after `systemd` on a systemd initramfs:
    # Plymouth needs whichever of those brings the devices up before it draws.
    if grep -qE '^HOOKS=.*\bsystemd\b' "$conf"; then
        sudo sed -i -E 's/^(HOOKS=.*\bsystemd\b)/\1 plymouth/' "$conf"
    else
        sudo sed -i -E 's/^(HOOKS=.*\budev\b)/\1 plymouth/' "$conf"
    fi
    grep -E '^HOOKS=' "$conf"
fi

echo "Rebuilding the boot image; this takes a minute."
sudo mkinitcpio -P

echo "Done. The Khadi splash shows from the next boot."
echo "To go back:  ./uninstall-boot-splash.sh"
