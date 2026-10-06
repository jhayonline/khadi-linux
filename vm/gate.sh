#!/usr/bin/env bash
# Khadi — Phase 1 gate check. Runs INSIDE the guest.
#
# The checklist from vm/README.md, mechanised. A gate you run by eye is a gate
# you pass by wishful thinking.
#
#   bash ~/khadi/vm/gate.sh
set -uo pipefail

: "${XDG_CONFIG_HOME:=$HOME/.config}"
pass=0; fail=0
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; pass=$((pass+1)); }
no()   { printf '  \033[31m✗\033[0m %s\n' "$*"; fail=$((fail+1)); }
note() { printf '    %s\n' "$*"; }

echo
echo "  KHADI — PHASE 1 GATE"
echo "  ────────────────────────────────────────────────────────"
echo

echo "  Packages"
while read -r p _; do
    [[ -z "$p" || "$p" == \#* ]] && continue
    pacman -Qq "$p" >/dev/null 2>&1 && ok "$p" || no "$p not installed"
done < "$HOME/khadi/packages.txt"
echo

echo "  Config linked"
for c in foot zellij btop hypr yazi fuzzel; do
    t="$XDG_CONFIG_HOME/$c"
    if [[ -e "$t" ]]; then ok "$c -> $(readlink -f "$t" | sed "s|$HOME|~|")"
    else no "$c missing"; fi
done
[[ -e "$XDG_CONFIG_HOME/khadi/theme.toml" ]] && ok "theme.toml" || no "theme.toml missing"
echo

echo "  Binaries on PATH"
for b in khadi-panel khadi-cheatsheet khadi-dev; do
    command -v "$b" >/dev/null && ok "$b" || no "$b not on PATH"
done
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
if   [[ "${cols:-0}" -ge 247 ]]; then ok "${W}x${H} -> ~$cols cols (faithful proportions)"
elif [[ "${cols:-0}" -ge 180 ]]; then ok "${W}x${H} -> ~$cols cols (usable, below 247)"
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
    kid=$(pgrep -x khadi-bar || pgrep -x mako || true)
    sp=$(tr '\0' '\n' < "/proc/${kid:-$$}/environ" 2>/dev/null | grep '^PATH=' | cut -d= -f2-)
    case ":$sp:" in
        *":$HOME/.local/bin:"*) ok "~/.local/bin on the SESSION PATH" ;;
        *) no "~/.local/bin missing from the session PATH — khadi-panel unfindable" ;;
    esac
    e=$(hyprctl configerrors 2>/dev/null | grep -cv '^[[:space:]]*$')
    [[ "$e" -eq 0 ]] && ok "no config errors" || no "$e config errors"
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
printf '  %d passed, %d failed\n\n' "$pass" "$fail"
[[ "$fail" -eq 0 ]]
