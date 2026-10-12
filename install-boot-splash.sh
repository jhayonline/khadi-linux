#!/bin/sh
# Puts the DEs-UI splash on the screen while this machine boots and shuts down, in
# place of the maker's logo. It is drawn by Plymouth, which the system already uses
# for that; this adds a theme for it and makes it the one in use.
#
# The pictures are made in the colours of the theme you use now; run this again after
# changing theme. To go back to what was there before:  ./uninstall-boot-splash.sh
#
# This rebuilds the boot image (the "initramfs"), which takes a minute.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
theme=/usr/share/plymouth/themes/des-ui

if ! [ -e /usr/lib/x86_64-linux-gnu/plymouth/script.so ] && ! find /usr/lib -name script.so -path '*plymouth*' 2>/dev/null | grep -q .; then
    echo "Plymouth's script plugin is missing; installing it." >&2
    sudo apt-get install -y plymouth-themes
fi

shell=/usr/local/bin/des-ui
[ -x "$shell" ] || shell="$here/target/release/des-ui"
if ! [ -x "$shell" ]; then
    echo "des-ui is not installed or built yet: run ./install.sh first." >&2
    exit 1
fi

pictures=$(mktemp -d)
trap 'rm -rf "$pictures"' EXIT
"$shell" --write-boot-splash "$pictures"

sudo install -d "$theme"
sudo install -m 644 "$pictures"/*.png "$here/session/boot/des-ui.plymouth" "$here/session/boot/des-ui.script" "$theme/"
sudo update-alternatives --install /usr/share/plymouth/themes/default.plymouth default.plymouth "$theme/des-ui.plymouth" 200
sudo update-alternatives --set default.plymouth "$theme/des-ui.plymouth"
echo "Rebuilding the boot image; this takes a minute."
sudo update-initramfs -u

echo "Done. The DEs-UI splash shows from the next boot."
echo "To go back:  ./uninstall-boot-splash.sh"
