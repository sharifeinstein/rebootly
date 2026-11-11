#!/bin/sh -e
# The rescue system's configuration overlay (apkovl) for mkimage.sh, based
# on aports' genapkovl-dhcp.sh. Adds Rebootly ($REBOOTLY_BIN), its start
# script ($REBOOTLY_START) on tty1 instead of a login, and no network at
# boot (the menu after Rebootly can start it, for a NAS).

HOSTNAME="$1"
if [ -z "$HOSTNAME" ]; then
	echo "usage: $0 hostname"
	exit 1
fi
: "${REBOOTLY_BIN:?set REBOOTLY_BIN to the static rebootly-cli}"
: "${REBOOTLY_START:?set REBOOTLY_START to rescue/alpine/rebootly-start}"

cleanup() {
	rm -rf "$tmp"
}

makefile() {
	OWNER="$1"
	PERMS="$2"
	FILENAME="$3"
	cat > "$FILENAME"
	chown "$OWNER" "$FILENAME"
	chmod "$PERMS" "$FILENAME"
}

rc_add() {
	mkdir -p "$tmp"/etc/runlevels/"$2"
	ln -sf /etc/init.d/"$1" "$tmp"/etc/runlevels/"$2"/"$1"
}

tmp="$(mktemp -d)"
trap cleanup EXIT

mkdir -p "$tmp"/etc
makefile root:root 0644 "$tmp"/etc/hostname <<EOT
$HOSTNAME
EOT

mkdir -p "$tmp"/etc/network
makefile root:root 0644 "$tmp"/etc/network/interfaces <<EOT
auto lo
iface lo inet loopback
EOT

mkdir -p "$tmp"/etc/apk
makefile root:root 0644 "$tmp"/etc/apk/world <<EOT
alpine-base
util-linux
cryptsetup
ntfs-3g
fuse-exfat
kbd
kbd-misc
kbd-bkeymaps
cifs-utils
EOT

# Filesystems and device-mapper, loaded at boot by the modules service.
makefile root:root 0644 "$tmp"/etc/modules <<EOT
loop
exfat
ntfs3
fuse
dm-mod
dm-crypt
usb-storage
uas
EOT

# Rebootly on tty1 instead of a login; shells on tty2 and tty3 (root, no
# password: it's a live system).
makefile root:root 0644 "$tmp"/etc/inittab <<EOT
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

tty1::respawn:/usr/local/bin/rebootly-start
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3

::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
EOT

makefile root:root 0644 "$tmp"/etc/motd <<EOT

Rebootly rescue. Rebootly runs on the first screen (Alt+F1); this is a shell.
  rebootly-cli volumes             list the disks
  rebootly-cli target /dev/sdb1    use a USB drive for writing
  rebootly-cli close               flush and unmount everything
  poweroff / reboot

EOT

mkdir -p "$tmp"/usr/local/bin
cp "$REBOOTLY_BIN" "$tmp"/usr/local/bin/rebootly-cli
cp "$REBOOTLY_START" "$tmp"/usr/local/bin/rebootly-start
chown root:root "$tmp"/usr/local/bin/rebootly-cli "$tmp"/usr/local/bin/rebootly-start
chmod 0755 "$tmp"/usr/local/bin/rebootly-cli "$tmp"/usr/local/bin/rebootly-start

rc_add devfs sysinit
rc_add dmesg sysinit
rc_add mdev sysinit
rc_add hwdrivers sysinit
rc_add modloop sysinit

rc_add hwclock boot
rc_add modules boot
rc_add sysctl boot
rc_add hostname boot
rc_add bootmisc boot
rc_add syslog boot

rc_add mount-ro shutdown
rc_add killprocs shutdown
rc_add savecache shutdown

tar -c -C "$tmp" etc usr | gzip -9n > "$HOSTNAME".apkovl.tar.gz
