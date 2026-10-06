#!/usr/bin/env bash
# Khadi — start a graphical session on tty1. Runs INSIDE the guest.
#
# TEST SCAFFOLDING. Khadi's real login surface is greetd + tuigreet (PLAN.md
# section 2), which is still unwritten. Until then this is the standard
# "no display manager" pattern: autologin on tty1, compositor from the shell
# profile. A compositor needs DRM master and a seat, so it cannot be started
# over ssh — CBackend::create() fails.
set -euo pipefail

MARK="# >>> khadi session >>>"
PROFILE="$HOME/.bash_profile"

echo "==> Autologin on tty1"
sudo mkdir -p /etc/systemd/system/getty@tty1.service.d
sudo tee /etc/systemd/system/getty@tty1.service.d/autologin.conf >/dev/null <<UNIT
[Service]
ExecStart=
ExecStart=-/sbin/agetty -o '-p -f -- \\\\u' --noclear --autologin $USER %I \$TERM
UNIT

echo "==> Compositor on tty1 login"
if ! grep -qsF "$MARK" "$PROFILE"; then
    cat >> "$PROFILE" <<PROF

$MARK
if [[ -z "\${WAYLAND_DISPLAY:-}" && "\$(tty)" == /dev/tty1 ]]; then
    # Headless QEMU gives the guest no 3D, so wlroots must software-render.
    # Fine here: Khadi is terminal-first and the gate cares about layout and
    # colour, not frame rate.
    export WLR_RENDERER=pixman
    export WLR_NO_HARDWARE_CURSORS=1
    export XDG_RUNTIME_DIR="/run/user/\$(id -u)"
    export XDG_CURRENT_DESKTOP=Hyprland XDG_SESSION_TYPE=wayland
    exec Hyprland > "\$HOME/hyprland.log" 2>&1
fi
# <<< khadi session <<<
PROF
    echo "  appended to $PROFILE"
else
    echo "  already present"
fi

sudo systemctl daemon-reload
echo
echo "==> Done. Reboot to land in the session:  sudo reboot"
