# Khadi test VM

A throwaway Arch VM. Two reasons it exists:

1. **It is the only safe place to run khadi-comp.** The compositor takes the
   display and the input devices. Starting one on the machine you are working
   on logs you out of it.
2. **It rehearses the claim that matters** — someone else runs one script on a
   machine that has never seen Khadi and gets the desktop. The guest builds
   from source for that reason. Pushing binaries from the host would be faster
   and would prove nothing: the host already has the toolchain, the libraries
   and the right versions of both.

Plain QEMU, no libvirt — one script, no daemon, nothing added to the host's
system state beyond two packages.

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
./vm/khadi-vm run                        # boots the installed disk now
KHADI_VM_USER=you ./vm/khadi-vm push     # copies the repo to ~/khadi
KHADI_VM_USER=you ./vm/khadi-vm ssh
```

Inside the guest:

```sh
~/khadi/vm/provision.sh
```

That installs the packages, builds the workspace, registers the session and
sets up greetd. Allow twenty minutes for the build: smithay and eframe are a
large dependency tree and this machine has two cores. Then `sudo systemctl
start greetd`, or reboot, and log in.

## Looking at it from the host

A login screen owns a VT and reads a keyboard, so it cannot be driven over
ssh. Two commands cover that:

```sh
KHADI_VM_USER=khadi ./vm/khadi-vm autologin   # session on tty1, no login
./vm/khadi-vm shot session.png                # the guest framebuffer, over QMP
./vm/khadi-vm type khadi ret                  # keystrokes, over QMP
```

`autologin` is test scaffolding and says so in the guest: it disables greetd,
sets an agetty override on tty1 and writes a marked block into both login
profiles. Khadi's own session manager asks for a password on purpose.

The block is **rewritten on every run**, not appended once. The version that
skipped the write when its markers were already present left a VM that had
been provisioned before the des-ui replacement still execing `start-hyprland`
at login — the new compositor installed, the old desktop booting over the top
of it, and nothing anywhere saying so.

## Knobs

| Variable | Default | |
| --- | --- | --- |
| `KHADI_VM_RAM` | `4G` | host has 11G total |
| `KHADI_VM_CPUS` | `2` | host has 4 |
| `KHADI_VM_DISK` | `24G` | sparse, grows as used |
| `KHADI_VM_SSH` | `2222` | host port forwarded to guest :22 |
| `KHADI_VM_USER` | `root` | set to your guest user after install |

## Why these choices

**UEFI, not BIOS.** Khadi's installer plan is archinstall with UEFI and LUKS
full-disk encryption. The VM should fail the same way real hardware would.

**virtio-gpu-gl with virgl.** khadi-comp drives the display through DRM and
renders with EGL. In this guest that resolves to `/dev/dri/card0` and
llvmpipe, which is software rendering: the desktop is correct but the frame
rate and the idle CPU reading are not representative of real hardware.

**SSH forwarded to 2222.** So the VM can be driven from the host without the
GUI, which makes failures inspectable instead of screenshot-only.

**Nothing is shared with the host.** No virtfs, no shared clipboard. The repo
gets in via `khadi-vm push`, which is a tar over ssh, and `push` deletes
`~/khadi` first so a renamed or deleted file cannot live on in the guest. A
test machine that can reach back into the host is not a test machine.

## What is not checked

There used to be a `gate.sh` here, 75 checks over the Hyprland desktop —
config links, `foot --check-config`, the yazi theme, bind discipline, and
Hyprland's loaded state. It went with the desktop it checked; it is in the
history at the tag `pre-des-ui`. Nothing has replaced it yet, so for now the
VM proves that provisioning works and that the session comes up, by eye and
by `shot`.

Still true from before, and still worth writing down: **a third party has not
run it.** The claim is "someone else runs the script and gets your desktop",
and the author's own VM is not someone else.
