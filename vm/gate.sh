#!/usr/bin/env bash
# Khadi — install gate. Runs INSIDE the guest.
#
# Phase 1's gate was "someone else runs the script and gets your desktop", and
# that is still what this answers; it has grown with the thing it gates.
#
# The checklist from vm/README.md, mechanised. A gate you run by eye is a gate
# you pass by wishful thinking.
#
#   bash ~/khadi/vm/gate.sh
#
# Run it from a LOGIN shell. The installer puts ~/.local/bin on PATH from the
# login profile, and `ssh host 'bash gate.sh'` reads neither that nor
# /etc/profile — so use `ssh host 'bash -lc "bash ~/khadi/vm/gate.sh"'`.
set -uo pipefail

: "${XDG_CONFIG_HOME:=$HOME/.config}"
pass=0; fail=0; skipped=0
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; pass=$((pass+1)); }
no()   { printf '  \033[31m✗\033[0m %s\n' "$*"; fail=$((fail+1)); }
# A skip is NOT a pass. It is counted and printed so that a run which checked
# nothing cannot read as a run that checked everything — the same reasoning
# behind khadi-check's --strict. This existed only as a call site until now:
# the no-Wayland branch below invoked `sk`, which was never defined, so it
# errored to stderr and the gate carried on as though nothing had happened.
sk()   { printf '  \033[2m–\033[0m %s\n' "$*"; skipped=$((skipped+1)); }
note() { printf '    %s\n' "$*"; }

echo
echo "  KHADI — INSTALL GATE"
echo "  ────────────────────────────────────────────────────────"
echo

echo "  Packages"
while read -r p _; do
    [[ -z "$p" || "$p" == \#* ]] && continue
    pacman -Qq "$p" >/dev/null 2>&1 && ok "$p" || no "$p not installed"
done < "$HOME/khadi/packages.txt"
echo

# The lists come from the INSTALLER, not from a copy kept here. A gate with its
# own list drifts from the thing it is gating, and it drifts in the direction
# that passes: the gate checked six config links while the installer made nine,
# so three could have been broken for a phase without a failing check.
CONFIGS=(); BINS=(); RUSTBINS=()
LISTS="$(grep -E '^(CONFIGS|BINS|RUSTBINS)=\(' "$HOME/khadi/bin/khadi-install")"
# Both definitions must be one line each and closed on it. A wrapped array
# would grep to a fragment, eval to a SHORTER list, and pass -- which is the
# same silent-drift failure this block exists to remove.
if [[ "$(grep -c . <<<"$LISTS")" -ne 3 ]] || grep -qv ')$' <<<"$LISTS"; then
    no "CONFIGS/BINS/RUSTBINS in khadi-install are not three single-line arrays — not parsing them"
else
    eval "$LISTS"
fi

echo "  Config linked"
if (( ${#CONFIGS[@]} == 0 )); then
    no "could not read CONFIGS from khadi-install — this gate is checking nothing"
else
    for c in "${CONFIGS[@]}"; do
        t="$XDG_CONFIG_HOME/$c"
        if [[ -e "$t" ]]; then ok "$c -> $(readlink -f "$t" | sed "s|$HOME|~|")"
        else no "$c missing"; fi
    done
fi
[[ -e "$XDG_CONFIG_HOME/khadi/theme.toml" ]] && ok "theme.toml" || no "theme.toml missing"
echo

echo "  Binaries on PATH"
if (( ${#BINS[@]} == 0 )); then
    no "could not read BINS from khadi-install — this gate is checking nothing"
else
    # RUSTBINS included deliberately. khadi-shell is the
    # components Khadi writes, the installer did not ship either, and this
    # gate passed anyway because khadi-vm push installs them by another route.
    for b in "${BINS[@]}" "${RUSTBINS[@]}"; do
        if command -v "$b" >/dev/null; then
            ok "$b"
        elif [[ -x "$HOME/.local/bin/$b" ]]; then
            # Installed, but this shell never sourced the profile the PATH
            # block went into. `ssh host 'bash gate.sh'` is neither a login
            # nor an interactive shell, so it reads neither — and all six
            # binaries then report "not on PATH" on a perfectly good install.
            no "$b installed but ~/.local/bin is not on this shell's PATH — run the gate from a login shell"
        else
            no "$b not on PATH and not in ~/.local/bin"
        fi
    done
fi
echo

# ON PATH IS NOT THE SAME AS WORKING. A stale khadi-hud passed every check
# here while three of the four panels were dead -- the layout showed
# `unknown command "net"` and the gate said 71/71, because nothing asked the
# binary to do anything. Only looking at the screen caught it.
#
# khadi-shell cannot be asked to draw into a pipe, so the equivalent is to run
# it and look at what it produced: a window, and a shell on the far end of its
# pty. The pty is the one worth checking by name -- it silently never spawned
# for a whole afternoon because Tauri 2 denies `listen` without a capability
# manifest and the rejected promise went nowhere.
# ASK THE BINARY WHETHER IT CAN READ THE MACHINE. This is the check the gate
# lost in the rewrite. Its sharpest test was always "on PATH is not the same as
# working" — a stale khadi-hud once passed 71/71 with three of four panels dead
# — and after khadi-shell replaced it the gate was down to "a window exists".
# The globe shipped upside down through exactly that gap.
echo "  The shell can read the machine"
if command -v khadi-shell >/dev/null; then
    if out=$(khadi-shell --selftest 2>&1); then
        while IFS= read -r line; do
            [[ "$line" == ok* ]] && ok "${line#ok   }"
        done <<<"$out"
    else
        while IFS= read -r line; do
            case "$line" in
                ok*)   ok "${line#ok   }" ;;
                FAIL*) no "${line#FAIL }" ;;
            esac
        done <<<"$out"
    fi
else
    no "khadi-shell not on PATH — cannot ask it to read anything"
fi
echo

echo "  The shell runs"
if ! command -v khadi-shell >/dev/null; then
    no "khadi-shell not on PATH — cannot ask it to run"
elif [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
    sk "khadi-shell (no Wayland display in this shell)"
else
    WEBKIT_DISABLE_DMABUF_RENDERER=1 setsid khadi-shell >/tmp/khadi-gate-shell.log 2>&1 &
    shell_pid=$!
    for _ in $(seq 1 20); do
        sleep 1
        pgrep -P "$shell_pid" -x bash >/dev/null 2>&1 && break
    done
    if pgrep -P "$shell_pid" -x bash >/dev/null 2>&1; then
        ok "khadi-shell spawned a shell on its pty"
    else
        no "khadi-shell started no pty: $(tail -1 /tmp/khadi-gate-shell.log)"
    fi
    if hyprctl clients -j 2>/dev/null | grep -q '"class": *"khadi-shell"'; then
        ok "khadi-shell mapped a window"
    else
        no "khadi-shell mapped no window"
    fi
    kill "$shell_pid" 2>/dev/null || true
fi
echo

echo "  Theme pipeline"
if [[ -x "$HOME/khadi/bin/khadi-theme" ]]; then
    t="$(sed -n 's/^name *= *"\(.*\)"/\1/p' "$XDG_CONFIG_HOME/khadi/theme.toml" | head -1)"
    ok "active theme: ${t:-unknown}"
    if (cd "$HOME/khadi" && ./bin/khadi-theme build "${t:-tron}" --check >/dev/null 2>&1); then
        ok "config/ matches templates/ — no drift"
    else
        no "config/ has drifted from templates/ — run khadi-theme build"
    fi
    n=$(ls "$HOME/khadi/themes"/*.toml 2>/dev/null | wc -l)
    [[ "$n" -ge 3 ]] && ok "$n themes available" || no "only $n theme(s); Phase 2 wants 3"
    # Every theme must render AND survive each app's own validator. This is
    # the rolling-release mitigation from section 11: upstream format changes
    # become a failing check rather than a user report.
    if (cd "$HOME/khadi" && ./bin/khadi-check >/dev/null 2>&1); then
        ok "all $n themes render and validate"
    else
        no "khadi-check failed — run it for detail"
    fi
else
    no "khadi-theme not installed"
fi
echo

echo "  Fonts"
if command -v khadi-fontcheck >/dev/null 2>&1; then
    # The check that a whole phase needed: fontconfig substitutes silently, so
    # "it rendered" says nothing about what it rendered with.
    if khadi-fontcheck "$XDG_CONFIG_HOME/khadi/theme.toml" >/dev/null 2>&1; then
        ok "every theme font resolves"
    else
        no "a theme font resolves to a substitute"
        khadi-fontcheck "$XDG_CONFIG_HOME/khadi/theme.toml" 2>&1 | grep '✗' | sed 's/^/    /'
    fi
else
    no "khadi-fontcheck not installed"
fi
echo

echo "  Config validity"
foot --check-config -c "$XDG_CONFIG_HOME/foot/foot.ini" 2>/dev/null \
    && ok "foot --check-config" || no "foot config rejected"
fuzzel --check-config --config="$XDG_CONFIG_HOME/fuzzel/fuzzel.ini" 2>/dev/null \
    && ok "fuzzel --check-config" || no "fuzzel config rejected"
python3 - <<'PY' && ok "yazi theme parses" || no "yazi theme broken"
import tomllib,os,sys
p=os.path.expanduser('~/.config/yazi/theme.toml')
try: tomllib.load(open(p,'rb'))
except Exception as e: print('   ',e); sys.exit(1)
PY
echo

echo "  Prompt"
# A/B item 13. The prompt is the one piece of Khadi that lives in a file the
# installer APPENDS to rather than replaces, so it is the one most likely to be
# half-applied — and a shell with no pill looks like a theme problem.
RC=""
for c in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.config/fish/conf.d/khadi-prompt.fish"; do
    grep -qsF '# >>> khadi prompt >>>' "$c" && RC="$c"
done
if [[ -n "$RC" ]]; then
    ok "prompt block in ${RC/#$HOME/\~}"
else
    no "no khadi prompt block in any interactive rc"
fi
if command -v starship >/dev/null 2>&1; then
    # starship exits 0 on a config it could not parse and warns on stderr, so
    # stderr is the exit status. Same reason khadi-check reads it that way.
    err="$(STARSHIP_CONFIG="$XDG_CONFIG_HOME/khadi/starship.toml" starship prompt 2>&1 >/dev/null)"
    [[ -z "$err" ]] && ok "starship renders the khadi config" \
                    || { no "starship rejected the installed config"; note "$err"; }
else
    no "starship not installed — the prompt is inert"
fi
echo

echo "  Keybinding discipline"
leak=$(grep -E '^bind' "$XDG_CONFIG_HOME/hypr/hyprland.conf" 2>/dev/null | grep -vE '\$mod' | wc -l)
[[ "$leak" -eq 0 ]] && ok "every Hyprland bind is Super-scoped" || no "$leak binds escape Super"
dup=$(grep -E '^bindm?\s*=' "$XDG_CONFIG_HOME/hypr/hyprland.conf" 2>/dev/null \
      | sed -E 's/^bindm?\s*=\s*//' \
      | awk -F',' '{gsub(/^ +| +$/,"",$1);gsub(/^ +| +$/,"",$2);print $1"|"$2}' \
      | sort | uniq -d | wc -l)
[[ "$dup" -eq 0 ]] && ok "no duplicate bindings" || no "$dup duplicate bindings"
grep -q 'pane_frame_style "none"' "$XDG_CONFIG_HOME/zellij/config.kdl" 2>/dev/null \
    && ok "zellij frames off" || no "zellij frames not disabled"
echo

echo "  Display"
read -r W H < <(tr ',' ' ' < /sys/class/graphics/fb0/virtual_size 2>/dev/null)
cols=$(( W * 72 / (10 * 96) * 2 ))
# 217, not 247. The threshold moved when khadi-hud replaced btop in the side
# columns: btop needed 60 for its CPU box, khadi-hud drew the same content in
# 34, and the columns went 84 -> 74. See PLAN.md section 10d.
if   [[ "${cols:-0}" -ge 217 ]]; then ok "${W}x${H} -> ~$cols cols (faithful proportions)"
elif [[ "${cols:-0}" -ge 180 ]]; then ok "${W}x${H} -> ~$cols cols (usable, below 217)"
else no "${W}x${H} -> ~$cols cols, layout needs >=180"; fi
echo

echo "  Compositor — loaded state, not just the file on disk"
if pgrep -x Hyprland >/dev/null 2>&1; then
    export XDG_RUNTIME_DIR="/run/user/$(id -u)"
    # ls | head -1 is alphabetical; stale instance dirs linger after a session
    # restart and every hyprctl call then silently targets a dead compositor.
    export HYPRLAND_INSTANCE_SIGNATURE="$(ls -1t "$XDG_RUNTIME_DIR/hypr" 2>/dev/null | head -1)"
    n=$(hyprctl binds 2>/dev/null | grep -c modmask)
    want=$(grep -c '^bind' "$XDG_CONFIG_HOME/hypr/hyprland.conf")
    [[ "$n" -eq "$want" ]] && ok "$n/$want binds actually loaded" \
                           || no "$n binds loaded, $want in the file — config not in effect"
    g=$(hyprctl getoption general:gaps_out 2>/dev/null | head -1 | grep -oE '[0-9]+' | head -1)
    [[ "$g" == "4" ]] && ok "gaps_out=4 in effect" || no "gaps_out=$g, expected 4 — defaults are live"
    kid=$(pgrep -x khadi-shell || pgrep -x mako || true)
    sp=$(tr '\0' '\n' < "/proc/${kid:-$$}/environ" 2>/dev/null | grep '^PATH=' | cut -d= -f2-)
    case ":$sp:" in
        *":$HOME/.local/bin:"*) ok "~/.local/bin on the SESSION PATH" ;;
        *) no "~/.local/bin missing from the session PATH — khadi-shell unfindable" ;;
    esac
    e=$(hyprctl configerrors 2>/dev/null | grep -cv '^[[:space:]]*$')
    [[ "$e" -eq 0 ]] && ok "no config errors" || no "$e config errors"

    # GUI apps. Khadi is terminal-first, not terminal-only, and this is the
    # difference between a browser that can open a file and one that cannot.
    # Both failures are silent at the app end, so the gate has to ask.
    if busctl --user list 2>/dev/null | grep -q org.freedesktop.portal.Desktop; then
        ok "xdg-desktop-portal is on the session bus"
    else
        no "no portal on the session bus — file pickers and screen sharing will fail"
    fi
    # The portal is D-Bus activated and inherits the ACTIVATION environment,
    # not the session's. Without XDG_CURRENT_DESKTOP in it, it never matches
    # hyprland-portals.conf and silently picks its own backend.
    if systemctl --user show-environment 2>/dev/null | grep -q '^XDG_CURRENT_DESKTOP='; then
        ok "XDG_CURRENT_DESKTOP is in the activation environment"
    else
        no "XDG_CURRENT_DESKTOP missing from the activation environment — hyprland-portals.conf will not match"
    fi
    cs=$(gsettings get org.gnome.desktop.interface color-scheme 2>/dev/null)
    case "$cs" in
        *prefer-dark*) ok "colour scheme is prefer-dark" ;;
        *) no "colour scheme is ${cs:-unset} — libadwaita apps will come up light" ;;
    esac
else
    note "Hyprland not running — loaded-state checks skipped"
fi
echo
echo "  Compositor"
if command -v Hyprland >/dev/null; then
    ok "Hyprland present ($(Hyprland --version 2>/dev/null | head -1 | awk '{print $2}'))"
    [[ -e /dev/dri/card0 ]] && ok "DRM device present" || no "no /dev/dri/card0"
    note "3D: $(grep -qi virgl /proc/modules && echo 'virgl module' || echo 'none — expect WLR_RENDERER=pixman')"
else no "Hyprland not installed"; fi
echo

echo "  ────────────────────────────────────────────────────────"
if (( skipped )); then
    printf '  %d passed, %d failed, %d skipped\n\n' "$pass" "$fail" "$skipped"
else
    printf '  %d passed, %d failed\n\n' "$pass" "$fail"
fi
[[ "$fail" -eq 0 ]]
