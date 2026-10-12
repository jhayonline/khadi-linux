# Not yet tried on real hardware

Things that are built and pass their automatic tests, but that nobody has yet
watched work on a real machine. Tick them off, or note what went wrong.

## Display options (Settings → Displays)

- [ ] Scale: 125%, 150%, 175%, 200% on the laptop panel
- [ ] Scale on an external monitor
- [ ] Turn: on its right side, upside down, on its left side — and whether right
      and left are the right way round
- [ ] Mirror the main display on a second monitor (same shape, and a different one)
- [ ] Turn a second monitor off, and back on
- [ ] The 15-second "keep this layout?" step undoes each of the above when left alone
- [ ] A kept layout comes back after logging out and in, and after unplugging and
      plugging the monitor
- [ ] The pointer, toasts and the screenshot chooser sit in the right place on a
      scaled or turned display
- [ ] A monitor turned off: whether its backlight goes dark too

## Screenshots and recording

- [ ] FULL SCREEN, AREA and WINDOW each save the right picture
- [ ] The letter keys in the chooser (F, A, W, R, Enter, Escape)
- [ ] The keyboard goes back to the application after the chooser closes
- [ ] Alt+Print and Shift+Print
- [ ] Copy to the clipboard (needs `wl-clipboard`)
- [ ] A real recording (needs `ffmpeg`): starts, shows `REC` in the status strip,
      stops, and plays back with the pointer in it
- [ ] The toast when `ffmpeg` is missing
- [ ] Screenshots and recordings of a second display

## Power and battery

- [ ] The power menu: Super+Escape and the power key
- [ ] Each action in it: lock, sleep, log out, restart, shut down
- [ ] The power key puts the machine to sleep when the screen is locked
- [ ] Turning the screen off when left alone, and back on at a key or the pointer
- [ ] Battery warnings at 15% and 8%
- [ ] Sleep at 4% after the one-minute warning
- [ ] Time left in the status strip, charging and discharging

## Earlier work with gaps

- [ ] Wireless data-control clipboard protocol (used by `wl-copy`)
- [ ] Media keys (play, next, previous) driving the music player

## Network and Bluetooth (Settings)

- [ ] Wired: a cable plugged in shows as connected; DISCONNECT and CONNECT work
- [ ] VPN: ADD brings in a WireGuard `.conf` and an OpenVPN `.ovpn` file
- [ ] VPN: CONNECT, DISCONNECT and FORGET
- [ ] Bluetooth: pairing a device that needs no code still works
- [ ] Bluetooth: a keyboard that shows a code to type on it
- [ ] Bluetooth: a phone that asks to confirm a code
- [ ] Bluetooth: a device that wants a PIN typed here
- [ ] Bluetooth: CANCEL while pairing

## Notifications: pictures and sounds

- [ ] The sound itself: one when a notification shows, a different one when urgent
- [ ] Several notifications at once make one sound, not several
- [ ] No sound with do not disturb on, or with sounds switched off in the settings
- [ ] Pictures from real applications: a browser's message with a sender's
      picture, a music player's cover

## Music: albums, artists, long lists

- [ ] Scrolling to the end of the liked songs, or of a long playlist, loads the rest
- [ ] ALBUMS in the sidebar; opening one; playing a track from it plays on through the album
- [ ] Clicking a track's artist, and its album (the album shows only when the view is wide)
- [ ] Albums and artists among search results
- [ ] BACK, through several lists
- [ ] The Spotify sign-in carried over from the old `edex-rs` folder still works

## Your picture (Settings → Session)

- [x] Clicking a picture sets it; it then shows in the frame above, on the lock
      screen, and on the login screen
- [x] USE with a typed path, including one starting with `~/`
- [x] REMOVE goes back to the pattern
- [x] A file that is no picture gives a "Picture not changed" notice

## Music: other devices, covers, repeat, like

- [ ] Controls while another device plays: pause, next, previous, seek, volume, shuffle
- [ ] Clicking a track while another device plays: it plays there
- [ ] DEVICES: moving the music to this machine, and from it to a phone
- [ ] Playing here after playing elsewhere: the `ON <DEVICE>` line goes away
- [ ] REPEAT in its three states, here and on another device
- [ ] LIKE: the approval it asks for the first time, then liking and unliking
- [ ] The large cover of what is playing on another device
- [ ] Scrolling a long list: covers appear as rows come into view

## The splash

- [ ] How the glitch reads in motion: how often, how hard
- [ ] It shows briefly on Super+Shift+R
- [ ] With two displays: it shows on each
- [x] After the password, the black before the splash is now about a second shorter
      (the compositor leaves without switching the panel off)
- [ ] Logging out still returns to the login screen, now that the compositor leaves at once
- [ ] `./install-boot-splash.sh`: the splash shows while booting and shutting down
- [ ] The boot splash's bar follows the boot, and the name glitches
- [ ] `./uninstall-boot-splash.sh` brings the old boot screen back

## Sound (Settings → Sound & Screen)

- [x] Clicking another output moves the sound there (speakers, Bluetooth, HDMI)
- [x] Clicking another input changes the microphone in use
- [x] The level sliders and MUTE, for the output, the input and each application
- [x] A device plugged in or connected while the page is open appears within a few seconds
- [x] Music that is playing follows a change of output

## Music on the lock screen

- [x] The card shows when something plays, here or on another device, and not otherwise
- [x] Previous, play/pause and next work with the pointer while locked
- [x] The media keys work while locked
- [x] On the real display the card sits clear of the password line

## Lyrics

- [x] LYRICS with a real track playing: the words appear and follow the singing
- [x] Clicking a line jumps to it
- [x] Scrolling by hand, then the view following again at the next line
- [x] A track whose words are not timed, and one with none
- [x] Words for a track playing on another device
