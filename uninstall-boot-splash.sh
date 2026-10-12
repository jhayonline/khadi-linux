#!/bin/sh
# Takes the Khadi splash off the boot screen again: what was shown before comes back.
set -eu

theme=/usr/share/plymouth/themes/khadi
conf=/etc/mkinitcpio.conf

# Back to whatever Plymouth's own default is. `spinner` ships with Plymouth on
# Arch, so there is always something to fall back to.
if command -v plymouth-set-default-theme >/dev/null; then
    sudo plymouth-set-default-theme spinner 2>/dev/null || true
fi
sudo rm -rf "$theme"

# Only the file install-boot-splash.sh saved is restored. A HOOKS line edited by
# hand since then is left alone: putting a stale copy back would undo that too.
if [ -f "$conf.khadi-backup" ]; then
    echo "Putting $conf back as it was."
    sudo cp "$conf.khadi-backup" "$conf"
    sudo rm -f "$conf.khadi-backup"
fi

echo "Rebuilding the boot image; this takes a minute."
sudo mkinitcpio -P
echo "Done. The boot screen is as it was before, from the next boot."
