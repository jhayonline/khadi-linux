# The setup form: every question Khadi asks a person to describe their machine.
#
# Sourced, never run. Two callers will use it — the first-boot owner setup
# (`khadi-provision-owner`) today, and the ISO's installer step when there is
# one — and they share this file rather than each keeping a copy. Omarchy learned
# that one the expensive way: its own note on the equivalent file says the two
# copies drifted and the keyboard list was already wrong in one of them.
#
# Each prompt reports one of three statuses, and every caller reads them alike:
#
#   0    the field is set; move on
#   1    Esc — unwind to the start of the form
#   130  Ctrl+C — a side channel the caller decides the meaning of
#
# Those three are the whole vocabulary because gum only ever exits on Esc or
# Ctrl+C, and in raw mode Ctrl+C arrives as a byte rather than a signal, so it
# never reaches the shell as SIGINT. Read the status; do not install a trap.
#
# Callers must provide `notice <message> <seconds>` for validation feedback, and
# own the variables these write: keyboard, keyboard_label, username, password,
# password_confirmation, full_name, email_address, hostname, timezone.

# The lists `gum choose` and `gum filter` read come through this rather than a
# pipe: a pipeline runs gum in a subshell, and the variable it sets there would
# not survive back into the prompt that needs it.
KHADI_FORM_TMP="${KHADI_FORM_TMP:-$(mktemp)}"

KHADI_FORM_BACK=1
KHADI_FORM_SIGNAL=130
# Not a gum status: this file's own, for "gum itself went wrong". See khadi_gum.
KHADI_FORM_FATAL=125

# English first, then the rest alphabetically. `gum choose` pages by --height and
# jumps to the page holding --selected, so an alphabetically-placed English (US)
# can land alone at the edge of a page nobody scanning for it reads.
KHADI_KEYBOARD_LAYOUTS=$'English (US)|us
English (UK)|uk
English (US, Dvorak)|dvorak
English (US, Colemak)|colemak
Belgian|be-latin1
Brazilian|br-abnt2
Bulgarian|bg-cp1251
Croatian|croat
Czech|cz
Danish|dk-latin1
Dutch|nl
Estonian|et
Finnish|fi
French|fr
French (Canada)|cf
French (Switzerland)|fr_CH
German|de
German (Switzerland)|de_CH-latin1
Greek|gr
Hebrew|il
Hungarian|hu
Icelandic|is-latin1
Irish|ie
Italian|it
Japanese|jp106
Latvian|lv
Lithuanian|lt
Norwegian|no-latin1
Polish|pl
Portuguese|pt-latin1
Romanian|ro
Russian|ru
Serbian|sr-latin
Slovak|sk-qwertz
Slovenian|slovene
Spanish|es
Spanish (Latin American)|la-latin1
Swedish|sv-latin1
Turkish|trq
Ukrainian|ua'

# useradd's own rule, near enough: start with a letter or underscore, then
# letters, digits, underscore or dash.
KHADI_USERNAME_PATTERN='^[a-z_][a-z0-9_-]*[$]?$'
# Names the system keeps for itself. Creating a login over one of these makes a
# second account with a privileged name and no obvious way back.
KHADI_RESERVED_USERNAMES='^(root|bin|daemon|mail|ftp|http|nobody|dbus|systemd-coredump|systemd-network|systemd-oom|systemd-journal-remote|systemd-resolve|systemd-timesync|tss|uuidd|alpm|git|avahi|cups|lp|polkitd|rtkit|qemu|brltty|rpc|libvirt-qemu|pcscd|greeter|_greetd)$'
# RFC 1123: 1-63 of letters, digits and dashes, not starting or ending with one.
KHADI_HOSTNAME_PATTERN='^[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?$'
KHADI_HOSTNAME_DEFAULT='khadi'

# Khadi's accent, so the prompts match the desktop they are about to start.
# gum's own defaults are a pink header and a red cursor, which belong to gum and
# not to this.
KHADI_FORM_ACCENT='#4FE3D8'
KHADI_FORM_MUTED='#6F8492'
# Separate arrays, because the subcommands do not take the same flags: `gum
# choose` has --cursor.foreground and `gum filter` does not. One shared array
# across both is how this file first shipped, and `gum filter` rejected the flag,
# exited 1, and the form read that as Esc and restarted for ever.
KHADI_CHOOSE_STYLE=(
  --header.foreground="$KHADI_FORM_ACCENT"
  --cursor.foreground="$KHADI_FORM_ACCENT"
)
KHADI_FILTER_STYLE=(
  --header.foreground="$KHADI_FORM_ACCENT"
  --indicator.foreground="$KHADI_FORM_ACCENT"
  --match.foreground="$KHADI_FORM_ACCENT"
)
KHADI_INPUT_STYLE=(
  --prompt.foreground="$KHADI_FORM_ACCENT"
)

# Runs a gum prompt and works out what its exit status meant, leaving what the
# person chose in KHADI_GUM_OUT.
#
# gum exits 1 for "Esc was pressed" and also for "that flag does not exist",
# which is a trap worth the few lines it costs to disarm: a mistyped style flag
# then looks exactly like somebody backing out, and a form that restarts on Esc
# restarts for ever with nothing on the screen to say why. Esc is silent and a
# usage error prints, so stderr is what tells them apart.
khadi_gum() {
  local err status
  err=$(mktemp) || return $KHADI_FORM_FATAL
  KHADI_GUM_OUT=$("$@" 2>"$err") && status=0 || status=$?
  if ((status != 0)) && [[ -s $err ]]; then
    local why; why="$(head -n 2 "$err" | tr '\n' ' ')"
    printf '\n  gum could not run: %s\n' "$why" >&2
    # Also to the caller's log when it has one. Without this the reason lands on
    # a tty that is about to be cleared, and the log says only that something
    # went wrong — which is the half of the message that does not help.
    [[ -n ${KHADI_FORM_LOG:-} ]] && printf '%s gum failed: %s\n' "$(date -Is)" "$why" >>"$KHADI_FORM_LOG"
    rm -f "$err"
    return $KHADI_FORM_FATAL
  fi
  rm -f "$err"
  return $status
}

# An installer's target is empty, so any name is free; first-boot setup replaces
# this, because its machine already has accounts on it.
khadi_username_taken() { return 1; }

# Written as `x=$(gum ...) && status=0 || status=$?` rather than an assignment
# followed by `status=$?`: a caller running under `set -e` would otherwise die on
# the failing assignment before it could read the status at all.

khadi_prompt_keyboard() {
  local choice status
  printf '%s\n' "$KHADI_KEYBOARD_LAYOUTS" | cut -d'|' -f1 >"$KHADI_FORM_TMP"
  khadi_gum gum choose "${KHADI_CHOOSE_STYLE[@]}" --height 10 --selected "English (US)" \
    --header "Keyboard layout" <"$KHADI_FORM_TMP"
  status=$?
  ((status == 0)) || return $status
  choice="$KHADI_GUM_OUT"

  keyboard_label="$choice"
  keyboard=$(printf '%s\n' "$KHADI_KEYBOARD_LAYOUTS" | awk -F'|' -v c="$choice" '$1==c{print $2; exit}')
}

khadi_prompt_username() {
  local status
  while true; do
    khadi_gum gum input "${KHADI_INPUT_STYLE[@]}" \
      --placeholder "Letters and digits, no spaces (like jhay)" --prompt "Username> "
    status=$?
    ((status == 0)) || return $status
    username="$KHADI_GUM_OUT"

    if [[ ! "$username" =~ $KHADI_USERNAME_PATTERN ]]; then
      notice "A username is lower-case letters and digits, with no spaces" 2
    elif [[ "$username" =~ $KHADI_RESERVED_USERNAMES ]]; then
      notice "That name belongs to the system" 2
    elif khadi_username_taken "$username"; then
      notice "There is already an account called that" 2
    else
      return 0
    fi
  done
}

khadi_prompt_password() {
  local status
  while true; do
    khadi_gum gum input "${KHADI_INPUT_STYLE[@]}" --password \
      --placeholder "For this account, for root, and to unlock the screen" --prompt "Password> "
    status=$?
    ((status == 0)) || return $status
    password="$KHADI_GUM_OUT"
    khadi_gum gum input "${KHADI_INPUT_STYLE[@]}" --password \
      --placeholder "The same again" --prompt "Confirm> "
    status=$?
    ((status == 0)) || return $status
    password_confirmation="$KHADI_GUM_OUT"

    if [[ -z "$password" ]]; then
      notice "The password cannot be empty: this unlocks the machine" 2
    elif [[ "$password" != "$password_confirmation" ]]; then
      notice "Those did not match" 2
    else
      return 0
    fi
  done
}

# Both may be left empty with Return, so an empty answer is a real answer and
# only Esc or Ctrl+C ends the prompt early.
khadi_prompt_identity() {
  local status
  khadi_gum gum input "${KHADI_INPUT_STYLE[@]}" \
    --placeholder "What git will sign your commits with (Return to skip)" --prompt "Full name> "
  status=$?
  ((status == 0)) || return $status
  full_name="$KHADI_GUM_OUT"
  khadi_gum gum input "${KHADI_INPUT_STYLE[@]}" \
    --placeholder "What git will sign your commits with (Return to skip)" --prompt "Email> "
  status=$?
  ((status == 0)) || return $status
  email_address="$KHADI_GUM_OUT"
  return 0
}

khadi_prompt_hostname() {
  local status
  while true; do
    khadi_gum gum input "${KHADI_INPUT_STYLE[@]}" \
      --placeholder "What this machine is called (Return for '$KHADI_HOSTNAME_DEFAULT')" --prompt "Hostname> "
    status=$?
    ((status == 0)) || return $status
    hostname="$KHADI_GUM_OUT"

    if [[ -z $hostname ]]; then
      hostname="$KHADI_HOSTNAME_DEFAULT"
      return 0
    elif [[ "$hostname" =~ $KHADI_HOSTNAME_PATTERN ]]; then
      return 0
    else
      notice "1 to 63 letters, digits or dashes, not starting or ending with a dash" 3
    fi
  done
}

# Guarded, and the fallback is a filter rather than a list: a machine being set
# up has often not joined a network yet, so the guess fails more often than it
# works, and under `set -e` an unguarded failure would end the form.
khadi_prompt_timezone() {
  local guess status
  guess=$(tzupdate -p 2>/dev/null) || guess=""

  if [[ -n $guess ]]; then
    timedatectl list-timezones >"$KHADI_FORM_TMP"
    khadi_gum gum choose "${KHADI_CHOOSE_STYLE[@]}" --height 10 --selected "$guess" \
      --header "Timezone" <"$KHADI_FORM_TMP"
    status=$?
  else
    # --fuzzy=false, which is not gum's default and matters here. Fuzzy matching
    # takes the query's letters in order and anywhere apart, case-insensitively,
    # so typing "Accra" offers "America/Costa_Rica" first — a·c·c·r·a really is
    # scattered through it — and Return puts the machine several thousand miles
    # from where it is. Substring matching makes "Accra" mean Accra.
    timedatectl list-timezones >"$KHADI_FORM_TMP"
    khadi_gum gum filter "${KHADI_FILTER_STYLE[@]}" --no-fuzzy --height 10 \
      --header "Timezone — type part of a city or region" <"$KHADI_FORM_TMP"
    status=$?
  fi
  ((status == 0)) || return $status
  timezone="$KHADI_GUM_OUT"

  [[ -n $timezone ]] || timezone="UTC"
}
