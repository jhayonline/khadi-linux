# Khadi test VM

A throwaway Arch VM for the Phase 1 gate: *someone else runs the script and
gets your desktop.* Plain QEMU, no libvirt — one script, no daemon, nothing
added to the host's system state beyond two packages.

## Once, on the host

```sh
sudo pacman -S --needed qemu-desktop edk2-ovmf
```

That is the only step needing root. `/dev/kvm` is already world-writable on
this machine, so no group changes and no re-login.

## Then

```sh
./vm/khadi-vm fetch     # Arch ISO, checksum-verified
./vm/khadi-vm create    # 24G sparse qcow2
./vm/khadi-vm run       # boots the installer
```

In the guest, install Arch as normal — `archinstall` is on the ISO. Choose a
**minimal** profile: no desktop, no greeter. Khadi supplies those. Say yes to
a normal user with sudo.

Then shut down, and from the host:

```sh
./vm/khadi-vm run                  # boots the installed disk now
./vm/khadi-vm push                 # copies the repo to ~/khadi in the guest
KHADI_VM_USER=you ./vm/khadi-vm ssh
```

Inside the guest:

```sh
~/khadi/vm/provision.sh
Hyprland
```

## Knobs

| Variable | Default | |
| --- | --- | --- |
| `KHADI_VM_RAM` | `4G` | host has 11G total |
| `KHADI_VM_CPUS` | `2` | host has 4 |
| `KHADI_VM_DISK` | `24G` | sparse, grows as used |
| `KHADI_VM_SSH` | `2222` | host port forwarded to guest :22 |
| `KHADI_VM_USER` | `root` | set to your guest user after install |

## Why these choices

**UEFI, not BIOS.** Khadi's real installer plan is archinstall with UEFI and
LUKS full-disk encryption (PLAN.md section 8). The VM should fail the same way
real hardware would.

**virtio-gpu-gl with virgl.** Hyprland wants a GPU. Without 3D the guest falls
back to `WLR_RENDERER=pixman`, which works but is slow — `provision.sh` prints
the incantation if you need it.

**SSH forwarded to 2222.** So the VM can be driven from the host without the
GUI, which makes failures inspectable instead of screenshot-only.

**Nothing is shared with the host.** No virtfs, no shared clipboard. The repo
gets in via `khadi-vm push`, which is a tar over ssh. A test machine that can
reach back into the host is not a test machine.

## Gate

Mechanised as `gate.sh` and run inside the guest:

```sh
bash ~/khadi/vm/gate.sh
```

**Last measured: 42 passed, 0 failed** — and that number is now out of date.
The gate has since grown: the config-link and binary lists are derived from
`khadi-install` rather than kept here (six links and three binaries were going
unchecked), and there is a new prompt section. It has not been re-run in the
VM since. On the author's host, against a throwaway `$HOME`, the new sections
pass.

It checks packages, config links,
`foot`/`fuzzel --check-config`, the yazi theme, `Super`-scope discipline,
duplicate bindings, frames-off, the column budget read from the real
framebuffer, and Hyprland's *loaded* state — binds actually registered, live
`gaps_out`, session PATH — rather than the config file on disk.

That last distinction matters: an earlier version of this gate passed a system
where only 6 of 36 binds were live, because it read the file instead of asking
the compositor.

What it cannot check:

- **A third party has not run it.** The gate's wording is "someone else runs
  the script and gets your desktop". The author's own VM is not that.
- Notification urgency variants — the test sent the same urgency three times.
- `tuigreet` runs on a bare VT with 16 ANSI colours, so it is the one surface
  where the palette can only be approximated.
