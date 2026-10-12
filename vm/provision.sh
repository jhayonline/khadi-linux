#!/usr/bin/env bash
# DEs-UI — guest provisioning. Runs INSIDE the VM, as the normal user, after
# Arch is installed and you have pushed the repo in with `des-ui-vm push`.
#
#   ~/des-ui/vm/provision.sh
#
# This is the rehearsal for the claim that matters: someone else runs one
# script on a machine that has never seen DEs-UI and gets the desktop. It
# builds from source on purpose. Pushing binaries from the host would be
# faster and would test nothing — the host already has a working toolchain,
# every library, and the right versions of both.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# --- the desktop that used to be here --------------------------------------
# DEs-UI was Hyprland plus a shell in a webview until the des-ui replacement.
# A VM provisioned before that still has its binaries, its greeter and its
# marked blocks in ~/.zshrc, and greetd still points at a des-ui-greet that no
# longer exists. Nothing in the repo removes them any more, because the script
# that knew how was deleted with the rest of it — so it is done here, once,
# and skipped on a machine that never had it.
if [[ -e /usr/local/bin/des-ui-shell || -e /usr/local/bin/des-ui-greet || -e /etc/des-ui ]]; then
    echo "==> Removing the previous DEs-UI"
    sudo systemctl disable --now greetd 2>/dev/null || true
    sudo rm -f /usr/local/bin/des-ui-shell /usr/local/bin/des-ui-greet \
               /usr/local/bin/des-ui-hud /usr/local/bin/des-ui-bar
    sudo rm -rf /etc/des-ui /usr/local/share/fonts/des-ui
    rm -rf ~/.config/hypr ~/.config/foot ~/.config/fuzzel ~/.config/mako \
           ~/.config/des-ui ~/.config/zellij ~/.config/yazi
    # The three marked blocks des-ui-install appended to the shell rc files.
    for rc in ~/.zshrc ~/.bashrc ~/.zprofile ~/.bash_profile; do
        [[ -f "$rc" ]] || continue
        python3 - "$rc" <<'STRIP'
import re, sys
p = sys.argv[1]; s = open(p).read()
open(p, 'w').write(
    re.sub(r'\n?# >>> des-ui (path|prompt|shell) >>>.*?# <<< des-ui \1 <<<\n', '', s, flags=re.S))
STRIP
    done
    sudo fc-cache -f >/dev/null 2>&1 || true
fi

echo
echo "==> Packages"
# One name per line, `#` to end of line is a comment — the same parse the
# README documents, so the two cannot drift.
mapfile -t PKGS < <(sed 's/#.*//' "$ROOT/packages.txt" | tr -s '[:space:]' '\n' | grep .)
echo "    ${#PKGS[@]} packages"
sudo pacman -Sy --needed --noconfirm "${PKGS[@]}"

echo
echo "==> Guest extras"
# openssh so the host can drive this; the host script forwards :2222 -> :22.
sudo pacman -S --needed --noconfirm openssh
sudo systemctl enable --now sshd

echo
echo "==> Building"
# smithay and eframe are a large dependency tree and this is a virtual machine
# with a few cores. Twenty minutes is normal; it is not stuck.
cd "$ROOT"
./install.sh

echo
echo "==> Login session"
# Arch ships no display manager, so nothing would offer the session. greetd
# with its own agreety is the smallest thing that gives a real login on a VT
# rather than an autologin that proves less than it looks.
sudo pacman -S --needed --noconfirm greetd greetd-agreety
sudo install -Dm644 /dev/stdin /etc/greetd/config.toml <<'CONF'
# DEs-UI, in the test VM. agreety is greetd's plain text greeter: it asks for a
# name and a password on the VT and then execs the session. DEs-UI had a
# graphical greeter of its own; it was dropped with the old codebase and is in
# the history at the tag pre-des-ui. Upstream lists a login screen under "not
# done yet", so until one exists this is the honest state of the art here.
[terminal]
vt = 1

[default_session]
command = "agreety --cmd /usr/local/bin/des-ui-session"
user = "greeter"
CONF
sudo systemctl enable greetd

echo
cat <<'NOTE'
==> Done

    sudo systemctl start greetd     # or just reboot

Log in at the prompt and the session starts. des-comp drives the display
itself through DRM; on this VM that is virtio-gpu. If it cannot, des-ui-session
falls back to the previous build and then to cage, and says which in
~/.local/state/des-ui/session.log.

Hold Super:

    Space  launcher      ,      settings     Tab  next application
    S      split         F      full screen  Q    close application
    [ ] \  collapse the left, right and bottom panels

Ctrl+Alt+F2 gets you a plain VT if the session wedges.
NOTE
