#!/bin/sh
# Takes the Khadi splash off the boot screen again: what was shown before comes back.
set -eu

theme=/usr/share/plymouth/themes/khadi
sudo update-alternatives --remove default.plymouth "$theme/khadi.plymouth" 2>/dev/null || true
sudo rm -rf "$theme"
echo "Rebuilding the boot image; this takes a minute."
sudo update-initramfs -u
echo "Done. The boot screen is as it was before, from the next boot."
