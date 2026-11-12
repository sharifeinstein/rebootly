use super::*;

// A laptop disk (GPT): EFI, Windows locked with BitLocker, a data NTFS
// partition already mounted read-write by the desktop; a USB stick (MBR,
// exFAT); the Alpine boot stick (vfat, not listed). Old-lsblk style
// strings for size/rm on the USB rows.
const LSBLK: &str = r#"{"blockdevices": [
 {"name":"/dev/nvme0n1","kname":"/dev/nvme0n1","size":512110190592,"fstype":null,"label":null,"mountpoint":null,"type":"disk","rm":false,"partuuid":null,"ptuuid":"0b0e…","pttype":"gpt","pkname":null},
 {"name":"/dev/nvme0n1p1","kname":"/dev/nvme0n1p1","size":104857600,"fstype":"vfat","label":"SYSTEM","mountpoint":null,"type":"part","rm":false,"partuuid":"aaaa","ptuuid":"x","pttype":"gpt","pkname":"/dev/nvme0n1"},
 {"name":"/dev/nvme0n1p3","kname":"/dev/nvme0n1p3","size":400000000000,"fstype":"BitLocker","label":null,"mountpoint":null,"type":"part","rm":false,"partuuid":"11111111-2222-3333-4444-555555555555","ptuuid":"x","pttype":"gpt","pkname":"/dev/nvme0n1"},
 {"name":"/dev/nvme0n1p4","kname":"/dev/nvme0n1p4","size":100000000000,"fstype":"ntfs","label":"Data","mountpoint":"/media/me/Data","type":"part","rm":false,"partuuid":"66666666-7777-8888-9999-AAAAAAAAAAAA","ptuuid":"x","pttype":"gpt","pkname":"/dev/nvme0n1"},
 {"name":"/dev/sdb1","kname":"/dev/sdb1","size":"62000000000","fstype":"exfat","label":"BACKUP","mountpoint":null,"type":"part","rm":"1","partuuid":"1234abcd-01","ptuuid":"1234abcd","pttype":"dos","pkname":"/dev/sdb"},
 {"name":"/dev/sdc1","kname":"/dev/sdc1","size":"8000000000","fstype":"vfat","label":"ALPINE","mountpoint":"/media/usb","type":"part","rm":"1","partuuid":null,"ptuuid":"beef","pttype":"dos","pkname":"/dev/sdc"}
]}"#;

fn listed(json: &str) -> Vec<Volume> {
    let rows = parse(json).unwrap();
    volumes(
        &rows,
        |m| m != "/media/me/Data",
        |k| (k == "/dev/sdb1").then_some(2048),
    )
}

#[test]
fn lists_windows_filesystems_with_state_and_identity() {
    let v = listed(LSBLK);
    let devs: Vec<&str> = v.iter().map(|v| v.device.as_str()).collect();
    assert_eq!(devs, ["/dev/nvme0n1p3", "/dev/nvme0n1p4", "/dev/sdb1"]);

    assert_eq!(v[0].state, VolumeState::Locked);
    assert_eq!(
        v[0].id,
        Some(VolumeId::gpt("11111111-2222-3333-4444-555555555555"))
    );

    assert_eq!(
        v[1].state,
        VolumeState::Mounted {
            root: "/media/me/Data".into(),
            read_only: false
        }
    );
    assert_eq!(
        v[1].id,
        Some(VolumeId::gpt("66666666-7777-8888-9999-aaaaaaaaaaaa"))
    );

    assert_eq!(v[2].state, VolumeState::Unmounted);
    assert!(v[2].removable);
    assert_eq!(v[2].size, Some(62_000_000_000));
    assert_eq!(
        v[2].id,
        Some(VolumeId::Mbr {
            signature: 0x1234_abcd,
            offset: 2048 * 512
        })
    );
}

#[test]
fn an_unlocked_bitlocker_partition_shows_its_mapping() {
    let json = LSBLK.replace(
        "\n]}",
        r#",
 {"name":"/dev/mapper/rebootly-nvme0n1p3","kname":"/dev/dm-0","size":399990000000,"fstype":"ntfs","label":"Windows","mountpoint":"/run/rebootly/nvme0n1p3","type":"crypt","rm":false,"partuuid":null,"ptuuid":null,"pttype":null,"pkname":"/dev/nvme0n1p3"}
]}"#,
    );
    let v = listed(&json);
    assert_eq!(v.len(), 3);
    assert_eq!(v[0].device, "/dev/nvme0n1p3");
    assert_eq!(v[0].fs.as_deref(), Some("ntfs"));
    assert_eq!(v[0].label.as_deref(), Some("Windows"));
    assert_eq!(
        v[0].state,
        VolumeState::Mounted {
            root: "/run/rebootly/nvme0n1p3".into(),
            read_only: true
        }
    );
    assert!(v[0].id.is_some());
}

#[test]
fn mountinfo_read_only_and_escapes() {
    let info = "36 27 8:4 / /media/me/My\\040Disk rw,relatime - ntfs3 /dev/sda4 rw\n\
                37 27 8:3 / /run/rebootly/sda3 ro,relatime - ntfs3 /dev/sda3 ro\n";
    assert!(!mount_read_only(info, "/media/me/My Disk"));
    assert!(mount_read_only(info, "/run/rebootly/sda3"));
    assert!(!mount_read_only(info, "/nope"));
}

#[test]
fn real_lsblk_output_parses() {
    // Whatever this machine has: the command and parser must agree.
    let run = crate::cmd::System;
    if !crate::cmd::Runner::has(&run, "lsblk") {
        return;
    }
    let out = crate::cmd::Runner::run(&run, "lsblk", LSBLK_ARGS, None).unwrap();
    if out.ok() {
        let rows = parse(&out.stdout).unwrap();
        assert!(
            rows.iter()
                .all(|r| !r.name.is_empty() && !r.kind.is_empty())
        );
    }
}

#[test]
fn blkid_fills_what_lsblk_left_empty() {
    let mut r = Row {
        name: "/dev/sda2".into(),
        kname: "/dev/sda2".into(),
        kind: "part".into(),
        ..Row::default()
    };
    fill_from_blkid(
        &mut r,
        "DEVNAME=/dev/sda2\nLABEL=Windows\nTYPE=ntfs\nPART_ENTRY_SCHEME=dos\n\
         PART_ENTRY_UUID=1234abcd-02\nPART_ENTRY_OFFSET=206848\n",
    );
    assert_eq!(r.fstype.as_deref(), Some("ntfs"));
    assert_eq!(r.label.as_deref(), Some("Windows"));
    let v = volumes(&[r], |_| true, |_| None);
    assert_eq!(
        v[0].id,
        Some(VolumeId::Mbr {
            signature: 0x1234_abcd,
            offset: 206_848 * 512
        })
    );

    let mut g = Row::default();
    fill_from_blkid(
        &mut g,
        "TYPE=BitLocker\nPART_ENTRY_SCHEME=gpt\nPART_ENTRY_UUID=AA-BB\n",
    );
    assert_eq!(identity(&g, None), Some(VolumeId::gpt("aa-bb")));
}
