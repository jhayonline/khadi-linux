#!/bin/sh
# Makes the DEs-UI login screen this machine's login screen, in place of GDM.
#
# It installs greetd (a small login manager), points it at the DEs-UI login screen,
# and switches the system from GDM to greetd. Nothing changes until the next boot,
# and ./uninstall-login.sh puts GDM back.
#
# If the machine ever comes up without a usable login, press Ctrl+Alt+F3 for a text
# console, sign in there, and run:
#     sudo systemctl disable greetd && sudo systemctl enable gdm3 && sudo reboot
set -eu
cd "$(dirname "$0")"

if ! /usr/local/bin/des-ui --help 2>/dev/null | grep -q -- --greeter; then
    echo "The installed des-ui has no login screen yet. Run ./install.sh first." >&2
    exit 1
fi
if ! /usr/local/bin/des-comp --help 2>/dev/null | grep -q -- --no-xwayland; then
    echo "The installed des-comp is too old to show the login screen. Run ./install.sh first." >&2
    exit 1
fi

sudo apt-get install -y greetd

config=/etc/greetd/config.toml
# The account greetd runs the login screen as, and the console it uses, are the
# distribution's choices: keep them.
greeter_user=$(sed -n 's/^user *= *"\(.*\)"/\1/p' "$config" 2>/dev/null | head -n 1)
greeter_user=${greeter_user:-_greetd}
vt=$(sed -n 's/^vt *= *\([0-9a-z"]*\).*/\1/p' "$config" 2>/dev/null | head -n 1)
vt=${vt:-7}
if ! id "$greeter_user" >/dev/null 2>&1; then
    echo "greetd's account '$greeter_user' does not exist; not changing anything." >&2
    exit 1
fi

# This desktop was called edex-rs before: bring its pictures along, and leave the old
# names pointing at the new ones for a login screen still running under them.
if [ -d /var/lib/edex-rs ] && [ ! -L /var/lib/edex-rs ] && [ ! -e /var/lib/des-ui ]; then
    sudo mv /var/lib/edex-rs /var/lib/des-ui
    sudo ln -s des-ui /var/lib/edex-rs
fi
if [ -e /usr/local/bin/edex-rs-greeter ]; then
    sudo ln -sfn des-ui-greeter /usr/local/bin/edex-rs-greeter
fi
if [ -f "$config.before-edex-rs" ] && [ ! -f "$config.before-des-ui" ]; then
    sudo mv "$config.before-edex-rs" "$config.before-des-ui"
fi

sudo install -Dm755 session/des-ui-greeter /usr/local/bin/des-ui-greeter
# Pictures for the login screen: each user can add their own, none can remove another's.
sudo install -d -m 1777 /var/lib/des-ui/avatars
# Where the login screen keeps its log, for when it misbehaves.
sudo install -d -o "$greeter_user" -m 755 /var/lib/des-ui/greeter

if [ -f "$config" ] && [ ! -f "$config.before-des-ui" ]; then
    sudo cp "$config" "$config.before-des-ui"
fi
sudo tee "$config" >/dev/null <<CONFIG
# Written by DEs-UI's install-login.sh. The previous file is config.toml.before-des-ui.
[terminal]
vt = $vt

[default_session]
command = "/usr/local/bin/des-ui-greeter"
user = "$greeter_user"
CONFIG

sudo systemctl disable gdm3 2>/dev/null || sudo systemctl disable gdm 2>/dev/null || true
sudo systemctl enable --force greetd

cat <<'DONE'

Done. The DEs-UI login screen appears from the next boot; nothing has changed yet.

  See it now, without signing in:   des-ui --greeter --windowed
  Use your own picture:             des-ui --set-avatar ~/Pictures/me.jpg
  Go back to GDM:                   ./uninstall-login.sh
  Its log, if it misbehaves:        /var/lib/des-ui/greeter/login.log

If the machine comes up without a usable login: Ctrl+Alt+F3, sign in, then
    sudo systemctl disable greetd && sudo systemctl enable gdm3 && sudo reboot
DONE
