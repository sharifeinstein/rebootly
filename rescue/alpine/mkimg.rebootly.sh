# Rebootly rescue image: a profile for Alpine's mkimage.sh (aports/scripts).
# shellcheck disable=SC2034,SC2153  # variables for (and ARCH from) mkimage.sh
# build.sh copies this file and genapkovl-rebootly.sh next to mkimage.sh.

profile_rebootly() {
	profile_standard
	title="Rebootly rescue"
	desc="Rebootly in a small Linux: back up a Windows PC that won't start."
	profile_abbrev="rebootly"
	image_name="rebootly-rescue"
	output_filename="rebootly-rescue-${ARCH}.iso"
	arch="x86_64 aarch64"
	# No extra kernel modules (standard adds xtables-addons for firewalls).
	kernel_addons=
	# Rebootly's tools (SPEC §7, §8): lsblk/blkid/mount (util-linux),
	# BitLocker (cryptsetup >= 2.3; dislocker is only in Alpine's testing
	# repository, so cryptsetup is the only unlock tool here), NTFS (the
	# kernel's ntfs3, fallback ntfs-3g), exFAT (kernel, fallback FUSE),
	# console font and keyboard layouts (kbd), NAS shares (cifs-utils).
	apks="$apks util-linux cryptsetup ntfs-3g fuse-exfat
		kbd kbd-misc kbd-bkeymaps cifs-utils"
	apkovl="genapkovl-rebootly.sh"
	hostname="rebootly"
}
