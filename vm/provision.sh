#!/usr/bin/env bash
# Khadi — guest provisioning. Runs INSIDE the VM, as the normal user, after
# Arch is installed and you have pushed the repo in with `khadi-vm push`.
#
#   ~/khadi/vm/provision.sh
#
# Installs what packages.txt asks for, then links the config. This is the
# Phase 1 gate rehearsal: if this does not produce a working desktop on a
# machine that has never seen Khadi, Phase 1 is not done.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "==> Packages"
mapfile -t PKGS < <(grep -vE '^\s*#|^\s*$' "$ROOT/packages.txt" | awk '{print $1}')
sudo pacman -S --needed --noconfirm "${PKGS[@]}"

echo
echo "==> Guest extras"
# openssh so the host can drive this; the host script forwards :2222 -> :22.
sudo pacman -S --needed --noconfirm openssh
sudo systemctl enable --now sshd

echo
echo "==> Khadi"
"$ROOT/bin/khadi-install" --apply

echo
cat <<'NOTE'
==> Graphics note

Hyprland needs a GPU. The host launches the VM with virtio-gpu-gl + virgl,
which usually gives the guest enough. If Hyprland refuses to start, fall back
to software rendering:

    export WLR_RENDERER=pixman
    export WLR_NO_HARDWARE_CURSORS=1
    Hyprland

Slow, but it renders, which is all the Phase 1 gate needs. Khadi is a
terminal-first desktop, so software rendering is less painful here than it
would be for a compositing-heavy setup.

==> Then

    Hyprland

Super+Return   khadi session      Super+/   cheatsheet
Super+Space    launcher           Super+E   files
NOTE
