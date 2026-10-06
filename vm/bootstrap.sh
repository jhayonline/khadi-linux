#!/usr/bin/env bash
# Khadi — guest bootstrap. Runs INSIDE the archiso live environment.
#
# A scripted base-Arch install, deliberately not archinstall: this needs to be
# deterministic and re-runnable, and archinstall's config schema moves between
# releases. Base system only — no desktop, no greeter. Khadi supplies those,
# and anything else here would contaminate the Phase 1 gate.
#
# Fetched and run from the host:
#   curl -fsSL http://10.0.2.2:8099/bootstrap.sh | bash
set -euo pipefail

DISK=/dev/vda
USERNAME="${KHADI_USER:-khadi}"
PASSWORD="${KHADI_PASS:-khadi}"     # test VM only
HOSTNAME=khadi-test
PUBKEY='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIML/nY/5CKlQPHd79eHGPQB5A6SHfQV2OzTaogRzLnA jhaycodes999@gmail.com'

say() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

[[ -b $DISK ]] || { echo "No $DISK — is this the VM?" >&2; exit 1; }
if mount | grep -q '/mnt'; then umount -R /mnt || true; fi

say "Partitioning $DISK (UEFI: 512M ESP + rest ext4)"
sgdisk --zap-all "$DISK"
sgdisk -n1:0:+512M -t1:ef00 -c1:EFI "$DISK"
sgdisk -n2:0:0     -t2:8300 -c2:root "$DISK"
partprobe "$DISK"; sleep 1
mkfs.fat -F32 -n EFI "${DISK}1"
mkfs.ext4 -F -L root "${DISK}2"

say "Mounting"
mount "${DISK}2" /mnt
mkdir -p /mnt/boot
mount "${DISK}1" /mnt/boot

say "pacstrap (base system only)"
pacstrap -K /mnt base linux linux-firmware \
    sudo openssh networkmanager \
    git base-devel vim

say "fstab"
genfstab -U /mnt >> /mnt/etc/fstab

say "Configuring"
arch-chroot /mnt /bin/bash -euo pipefail <<CHROOT
ln -sf /usr/share/zoneinfo/UTC /etc/localtime
hwclock --systohc
sed -i 's/^#en_US.UTF-8/en_US.UTF-8/' /etc/locale.gen
locale-gen
echo 'LANG=en_US.UTF-8' > /etc/locale.conf
echo '$HOSTNAME' > /etc/hostname
cat > /etc/hosts <<HOSTS
127.0.0.1   localhost
::1         localhost
127.0.1.1   $HOSTNAME.localdomain $HOSTNAME
HOSTS

useradd -m -G wheel -s /bin/bash '$USERNAME'
echo '$USERNAME:$PASSWORD' | chpasswd
echo 'root:$PASSWORD' | chpasswd
echo '%wheel ALL=(ALL:ALL) NOPASSWD: ALL' > /etc/sudoers.d/wheel
chmod 440 /etc/sudoers.d/wheel

# SSH with the host's key, so the host can drive this guest without the GUI.
install -d -m700 -o '$USERNAME' -g '$USERNAME' /home/$USERNAME/.ssh
echo '$PUBKEY' > /home/$USERNAME/.ssh/authorized_keys
chown $USERNAME:$USERNAME /home/$USERNAME/.ssh/authorized_keys
chmod 600 /home/$USERNAME/.ssh/authorized_keys
systemctl enable sshd
systemctl enable NetworkManager

# UEFI boot, matching the real installer plan (PLAN.md section 8).
bootctl install
ROOT_UUID=\$(blkid -s UUID -o value ${DISK}2)
cat > /boot/loader/loader.conf <<LOADER
default khadi.conf
timeout 1
console-mode max
LOADER
cat > /boot/loader/entries/khadi.conf <<ENTRY
title   Khadi test
linux   /vmlinuz-linux
initrd  /initramfs-linux.img
options root=UUID=\$ROOT_UUID rw
ENTRY
CHROOT

say "Done"
cat <<DONE

  User     : $USERNAME  (password: $PASSWORD)
  SSH      : key-based from the host, already installed
  Next     : poweroff, then on the host:

      ./vm/khadi-vm run
      KHADI_VM_USER=$USERNAME ./vm/khadi-vm push
      KHADI_VM_USER=$USERNAME ./vm/khadi-vm ssh

DONE
