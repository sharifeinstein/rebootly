use super::writer::HiveWriter;
use super::*;

fn sample() -> HiveWriter {
    let mut w = HiveWriter::new("ROOT");
    w.key(r"Software\Microsoft\Windows")
        .sz("Plain", "hello")
        .expand_sz("Path", r"%USERPROFILE%\Documents")
        .dword("Small", 0x1234_5678);
    w.key(r"Software\Zeta");
    w.key(r"Software\alpha");
    w.key("Ünïcode\u{4e2d}文").sz("", "default value");
    w
}

#[test]
fn roundtrip_keys_and_values() {
    let hive = Hive::from_bytes(sample().to_bytes()).unwrap();
    assert!(hive.header().checksum_ok);
    assert!(!hive.header().seq_mismatch());
    let k = hive
        .open_key(r"SOFTWARE\microsoft\WINDOWS")
        .unwrap()
        .unwrap();
    assert_eq!(k.name(), "Windows");
    assert_eq!(k.string("plain").unwrap().as_deref(), Some("hello"));
    assert_eq!(
        k.string("Path").unwrap().as_deref(),
        Some(r"%USERPROFILE%\Documents")
    );
    let small = k.value("Small").unwrap().unwrap();
    assert_eq!(small.as_dword(), Some(0x1234_5678));
    assert_eq!(small.as_string(), None);
}

#[test]
fn subkeys_listed_and_missing_paths_are_none() {
    let hive = Hive::from_bytes(sample().to_bytes()).unwrap();
    let sw = hive.open_key("Software").unwrap().unwrap();
    let mut names: Vec<_> = sw
        .subkeys()
        .unwrap()
        .iter()
        .map(|k| k.name().to_owned())
        .collect();
    names.sort_by_key(|n| n.to_uppercase());
    assert_eq!(names, ["alpha", "Microsoft", "Zeta"]);
    assert!(hive.open_key(r"Software\Nope\Deeper").unwrap().is_none());
    assert!(sw.value("absent").unwrap().is_none());
}

#[test]
fn utf16_key_names_and_default_value() {
    let hive = Hive::from_bytes(sample().to_bytes()).unwrap();
    let k = hive.open_key("ünïcode\u{4e2d}文").unwrap().unwrap();
    assert_eq!(k.string("").unwrap().as_deref(), Some("default value"));
}

#[test]
fn multi_sz_decodes() {
    let mut w = HiveWriter::new("R");
    let data: Vec<u8> = "a\0bc\0\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    w.key("K").set("M", ValueType::MultiSz, data);
    let hive = Hive::from_bytes(w.to_bytes()).unwrap();
    let v = hive
        .open_key("K")
        .unwrap()
        .unwrap()
        .value("M")
        .unwrap()
        .unwrap();
    assert_eq!(v.as_multi_sz().unwrap(), ["a", "bc"]);
}

#[test]
fn rejects_non_hives() {
    assert!(matches!(
        Hive::from_bytes(vec![0; 10]),
        Err(HiveError::NotAHive)
    ));
    assert!(matches!(
        Hive::from_bytes(vec![0; 8192]),
        Err(HiveError::NotAHive)
    ));
}

#[test]
fn corrupted_bytes_never_panic() {
    let good = sample().to_bytes();
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..3000 {
        let mut bad = good.clone();
        for _ in 0..8 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let pos = (seed as usize) % bad.len();
            bad[pos] = (seed >> 32) as u8;
        }
        if let Ok(h) = Hive::from_bytes(bad) {
            walk(&h.root().unwrap(), 0);
        }
    }
}

fn walk(k: &Key<'_>, depth: usize) {
    if depth > 16 {
        return;
    }
    let _ = k.values();
    if let Ok(subs) = k.subkeys() {
        for s in subs {
            walk(&s, depth + 1);
        }
    }
}

#[test]
fn truncated_file_is_an_error_not_a_panic() {
    let good = sample().to_bytes();
    for cut in [4096, 4100, 4200, good.len() - 100] {
        let _ = Hive::from_bytes(good[..cut].to_vec()).map(|h| walk(&h.root().unwrap(), 0));
    }
}

#[test]
fn dirty_detection_seq_and_logs() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("NTUSER.DAT");

    let mut w = sample();
    std::fs::write(&p, w.to_bytes()).unwrap();
    std::fs::write(dir.path().join("ntuser.dat.LOG1"), vec![0u8; 8192]).unwrap();
    let h = Hive::open(&p).unwrap();
    let st = dirty::assess(&p, h.header());
    assert!(!st.is_dirty());
    assert_eq!(st.logs.len(), 1);
    assert_eq!(st.logs[0].format, dirty::LogFormat::Empty);

    w.sequences(7, 6);
    std::fs::write(&p, w.to_bytes()).unwrap();
    let h = Hive::open(&p).unwrap();
    assert!(dirty::assess(&p, h.header()).is_dirty());

    w.sequences(9, 9);
    std::fs::write(&p, w.to_bytes()).unwrap();
    std::fs::write(dir.path().join("NTUSER.DAT.LOG2"), w.pending_log_bytes(9)).unwrap();
    let h = Hive::open(&p).unwrap();
    let st = dirty::assess(&p, h.header());
    assert!(!st.seq_mismatch);
    assert!(st.is_dirty());
    let pending: Vec<_> = st.logs.iter().filter(|l| l.pending).collect();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].newest_pending_seq, Some(9));

    // An old entry (seq below the hive's) is already applied: not dirty.
    std::fs::write(dir.path().join("NTUSER.DAT.LOG2"), w.pending_log_bytes(3)).unwrap();
    assert!(!dirty::assess(&p, h.header()).is_dirty());
}
