use super::key::{GENERIC, KMS, encode};
use super::*;
use crate::hive::ValueType;
use crate::hive::writer::HiveWriter;

fn utf16(parts: &[&str]) -> Vec<u8> {
    // DigitalProductId4-like: a size, then NUL-padded UTF-16 fields.
    let mut v = vec![0xF8, 0x04, 0, 0, 4, 0, 1, 0];
    for p in parts {
        for u in p.encode_utf16().chain(std::iter::repeat_n(0, 8)) {
            v.extend(u.to_le_bytes());
        }
    }
    v
}

fn software(key: Option<&str>, channel: Option<&str>, backup_key: Option<&str>) -> Hive {
    let mut w = HiveWriter::new("ROOT");
    let cv = w.key(CURRENT_VERSION);
    cv.sz("EditionID", "Professional");
    if let Some(k) = key {
        cv.set("DigitalProductId", ValueType::Binary, encode(k));
    }
    if let Some(c) = channel {
        cv.set(
            "DigitalProductId4",
            ValueType::Binary,
            utf16(&[
                "03612-04900-000-000000-00-1033-19045.0000-3172025",
                "{5f4b}",
                c,
            ]),
        );
    }
    if let Some(b) = backup_key {
        w.key(SPP).sz("BackupProductKeyDefault", b);
    }
    Hive::from_bytes(w.to_bytes()).unwrap()
}

const RETAIL_KEY: &str = "BBBBB-BBBBB-BBBBB-BBBBB-BBBNB";

#[test]
fn kinds() {
    let kind = |k, c| read(&software(k, c, None)).unwrap().unwrap().kind();
    assert_eq!(kind(Some(GENERIC[4]), Some("Retail")), Kind::Digital);
    assert_eq!(kind(Some(RETAIL_KEY), Some("Retail")), Kind::Retail);
    assert_eq!(kind(Some(RETAIL_KEY), Some("OEM:DM")), Kind::Firmware);
    assert_eq!(kind(Some(RETAIL_KEY), Some("OEM:NONSLP")), Kind::Oem);
    assert_eq!(kind(Some(RETAIL_KEY), Some("Volume:MAK")), Kind::Mak);
    assert_eq!(kind(Some(RETAIL_KEY), Some("Volume:GVLK")), Kind::Kms);
    assert_eq!(kind(Some(KMS[4]), None), Kind::Kms);
    assert_eq!(kind(Some(RETAIL_KEY), None), Kind::Unknown);
    assert_eq!(kind(None, Some("Retail")), Kind::Digital);
    assert!(read(&software(None, None, None)).unwrap().is_none());
}

#[test]
fn backup_key_when_no_digital_product_id() {
    let l = read(&software(None, None, Some(GENERIC[0])))
        .unwrap()
        .unwrap();
    assert_eq!(l.kind(), Kind::Digital);
    assert_eq!(l.edition.as_deref(), Some("Professional"));
}

#[test]
fn key_stays_out_of_the_checklist() {
    let l = read(&software(Some(RETAIL_KEY), Some("Retail"), None))
        .unwrap()
        .unwrap();
    for bundle in [true, false] {
        let s = l.manual_step(bundle);
        let all = format!("{} {}", s.title, s.detail);
        assert!(!all.contains(RETAIL_KEY), "{all}");
        assert!(all.contains("ending BBBNB"), "{all}");
        assert!(all.contains("(Professional)"), "{all}");
        assert_eq!(all.contains("in sensitive.age"), bundle, "{all}");
    }
    assert_eq!(
        l.summary(),
        "retail key, key ending BBBNB (Professional, channel Retail)"
    );
    let text = l.bundle_text("OLD-PC").unwrap();
    assert!(text.contains(&format!("Key: {RETAIL_KEY}")), "{text}");
    assert!(text.contains("Channel: Retail"), "{text}");

    let digital = read(&software(Some(GENERIC[4]), Some("Retail"), None))
        .unwrap()
        .unwrap();
    assert!(digital.bundle_text("OLD-PC").is_none());
    assert!(
        !digital
            .manual_step(false)
            .detail
            .contains("encrypted bundle")
    );
}

#[test]
fn channel_found_by_value() {
    assert_eq!(
        channel_of(&utf16(&["x", "OEM:DM", "y"])).as_deref(),
        Some("OEM:DM")
    );
    assert_eq!(
        channel_of(&utf16(&["Volume:MAK"])).as_deref(),
        Some("Volume:MAK")
    );
    assert_eq!(
        channel_of(&utf16(&["OEM:DM2", "OEM:ABCDEFGHIJKLMNOP"])),
        None
    );
    assert_eq!(channel_of(&utf16(&["Professional"])), None);
}
