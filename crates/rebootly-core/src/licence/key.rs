//! Windows product keys: decoding `DigitalProductId` (Windows 8 and later
//! layout) and the generic keys every PC of an edition shares.

use age::secrecy::{ExposeSecret, SecretString};

const CHARS: &[u8; 24] = b"BCDFGHJKMPQRTVWXY2346789";
/// The 15 key bytes start here in `DigitalProductId`.
const KEY_OFFSET: usize = 52;

/// A decoded 25-character key, `XXXXX-XXXXX-XXXXX-XXXXX-XXXXX`. Debug is
/// redacted: the key goes only into the encrypted bundle.
pub struct ProductKey(SecretString);

impl ProductKey {
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
    /// The last group, as `slmgr /dli` and Settings show it ("partial key").
    pub fn last5(&self) -> &str {
        let k = self.expose();
        &k[k.len() - 5..]
    }
    /// One of the generic keys Windows installs when there is no key of its
    /// own (digital licence, firmware key not used, upgrade).
    pub fn is_generic(&self) -> bool {
        GENERIC.contains(&self.expose())
    }
    pub fn is_kms(&self) -> bool {
        KMS.contains(&self.expose())
    }
}

impl std::fmt::Debug for ProductKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProductKey(*****-…-{})", self.last5())
    }
}

/// Decode the key in a `DigitalProductId` value (Windows 8 and later; an
/// all-zero key, as some volume installs have, gives `None`).
pub fn decode(digital_product_id: &[u8]) -> Option<ProductKey> {
    let mut key: [u8; 15] = digital_product_id
        .get(KEY_OFFSET..KEY_OFFSET + 15)?
        .try_into()
        .ok()?;
    if key.iter().all(|&b| b == 0) {
        return None;
    }
    // Bit 3 of the top byte marks the Windows 8+ layout (the often-copied
    // `(b / 6) & 1` gets it wrong when the N is late in the key).
    let win8 = key[14] & 0x08 != 0;
    key[14] &= 0xF7;
    let mut out = [0u8; 25];
    let mut last = 0usize;
    for slot in out.iter_mut().rev() {
        let mut cur = 0u32;
        for b in key.iter_mut().rev() {
            cur = cur * 256 + u32::from(*b);
            *b = (cur / 24) as u8;
            cur %= 24;
        }
        *slot = CHARS[cur as usize];
        last = cur as usize;
    }
    let mut s: Vec<u8> = out.to_vec();
    if win8 {
        // The first character's index says where the N goes; it replaces
        // that first character.
        s.remove(0);
        s.insert(last, b'N');
    }
    let plain = String::from_utf8(s).ok()?;
    Some(ProductKey(SecretString::from(group(&plain))))
}

/// A key written out as text (`BackupProductKeyDefault`): five groups of
/// five key characters, one of them N.
pub fn parse(text: &str) -> Option<ProductKey> {
    let t = text.trim().to_ascii_uppercase();
    let groups: Vec<&str> = t.split('-').collect();
    let ok = groups.len() == 5
        && groups
            .iter()
            .all(|g| g.len() == 5 && g.bytes().all(|c| c == b'N' || CHARS.contains(&c)))
        && t.matches('N').count() == 1
        // The N goes in at a position 0..=23: it is never the last character.
        && !t.ends_with('N');
    ok.then(|| ProductKey(SecretString::from(t)))
}

fn group(k: &str) -> String {
    k.as_bytes()
        .chunks(5)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("-")
}

/// Generic (default) keys of Windows 10 and 11 by edition: they install an
/// edition but activate nothing themselves (digital licence, or the
/// firmware key wasn't used). Retail-channel defaults, then the OEM
/// Activation 3.0 defaults.
pub const GENERIC: &[&str] = &[
    "YTMG3-N6DKC-DKB77-7M9GH-8HVX7", // Home
    "4CPRK-NM3K3-X6XXQ-RXX86-WXCHW", // Home N
    "BT79Q-G7N6G-PGBYW-4YWX6-6F4BT", // Home Single Language
    "N2434-X9D7W-8PF6X-8DV9T-8TYMD", // Home Country Specific
    "VK7JG-NPHTM-C97JM-9MPGT-3V66T", // Pro
    "2B87N-8KFHP-DKV6R-Y2C8J-PKCKT", // Pro N
    "DXG7C-N36C4-C4HTG-X4T3X-2YV77", // Pro for Workstations
    "WYPNQ-8C467-V2W6J-TX4WX-WT2RQ", // Pro for Workstations N
    "8PTT6-RNW4C-6V7J2-C2D3X-MHBPB", // Pro Education
    "YNMGQ-8RYV3-4PGQ3-C8XTP-7CFBY", // Education
    "XGVPP-NMH47-7TTHJ-W3FW7-8HV2C", // Enterprise
    "37GNV-YCQVD-38XP9-T848R-FC2HD", // Home (OEM)
    "33CY4-NPKCC-V98JP-42G8W-VH636", // Home N (OEM)
    "NTRHT-XTHTG-GBWCG-4MTMP-HH64C", // Single Language (OEM)
    "7B6NC-V3438-TRQG7-8TCCX-H6DDY", // China Single Language (OEM)
    "NF6HC-QH89W-F8WYV-WWXV4-WFG6P", // Pro (OEM)
    "NH7W7-BMC3R-4W9XT-94B6D-TCQG3", // Pro N (OEM)
];

/// KMS client keys (GVLK): the PC activates against an organisation's KMS
/// server. Usually also recognisable by the `Volume:GVLK` channel.
pub const KMS: &[&str] = &[
    "TX9XD-98N7V-6WMQ6-BX7FG-H8Q99", // Home
    "3KHY7-WNT83-DGQKR-F7HPR-844BM", // Home N
    "7HNRX-D7KGG-3K4RQ-4WPJ4-YTDFH", // Home Single Language
    "PVMJN-6DFY6-9CCP6-7BKTT-D3WVR", // Home Country Specific
    "W269N-WFGWX-YVC9B-4J6C9-T83GX", // Pro
    "MH37W-N47XK-V7XM9-C7227-GCQG9", // Pro N
    "NRG8B-VKK3Q-CXVCJ-9G2XF-6Q84J", // Pro for Workstations
    "9FNHH-K3HBT-3W4TD-6383H-6XYWF", // Pro for Workstations N
    "6TP4R-GNPTD-KYYHQ-7B7DP-J447Y", // Pro Education
    "NW6C2-QMPVW-D7KKK-3GKT6-VCFB2", // Education
    "NPPR9-FWDCX-D2C8J-H872K-2YT43", // Enterprise
];

/// Inverse of [`decode`], for tests and fixtures.
#[cfg(any(test, feature = "fixtures"))]
pub fn encode(key: &str) -> Vec<u8> {
    // drop the N (remember where), put the
    // N's position first, base-24 → 15 bytes, set the Windows 8 flag.
    let plain: Vec<u8> = key.bytes().filter(|&c| c != b'-').collect();
    let n = plain
        .iter()
        .position(|&c| c == b'N')
        .expect("Windows 8+ key");
    let mut digits: Vec<u8> = vec![n as u8];
    digits.extend(
        plain
            .iter()
            .filter(|&&c| c != b'N')
            .map(|c| CHARS.iter().position(|x| x == c).unwrap() as u8),
    );
    let mut bytes = [0u8; 15];
    for d in digits {
        let mut carry = u32::from(d);
        for b in bytes.iter_mut() {
            let v = u32::from(*b) * 24 + carry;
            *b = (v & 0xFF) as u8;
            carry = v >> 8;
        }
    }
    bytes[14] |= 0x08;
    let mut id = vec![0u8; 164];
    id[KEY_OFFSET..KEY_OFFSET + 15].copy_from_slice(&bytes);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_what_was_encoded() {
        for k in GENERIC.iter().chain(KMS).chain(&[
            "BBBBB-BBBBB-BBBBB-BBBBB-BBBNB",
            "NBBBB-BBBBB-BBBBB-BBBBB-BBBBB",
        ]) {
            let d = decode(&encode(k)).unwrap();
            assert_eq!(d.expose(), *k);
            assert_eq!(d.last5(), &k[24..]);
        }
        assert!(decode(&encode(GENERIC[4])).unwrap().is_generic());
        assert!(decode(&encode(KMS[0])).unwrap().is_kms());
        assert!(
            !decode(&encode("BBBBB-BBBBB-BBBBB-BBBBB-BBBNB"))
                .unwrap()
                .is_generic()
        );
    }

    #[test]
    fn parses_written_keys() {
        assert_eq!(
            parse(" vk7jg-nphtm-c97jm-9mpgt-3v66t\n").unwrap().expose(),
            GENERIC[4]
        );
        assert!(parse("VK7JG-NPHTM-C97JM-9MPGT").is_none());
        assert!(parse("VK7JG-NPHTM-C97JM-9MPGT-3V66A").is_none()); // A isn't a key letter
        assert!(parse("BBBBB-BBBBB-BBBBB-BBBBB-BBBBB").is_none()); // no N
        assert!(parse("BBBBB-BBBBB-BBBBB-BBBBB-BBBBN").is_none()); // N can't be last
    }

    #[test]
    fn missing_or_empty_key() {
        assert!(decode(&[0u8; 40]).is_none());
        assert!(decode(&[0u8; 164]).is_none());
    }

    #[test]
    fn debug_hides_the_key() {
        let d = decode(&encode(GENERIC[4])).unwrap();
        assert_eq!(format!("{d:?}"), "ProductKey(*****-…-3V66T)");
    }

    #[test]
    fn generic_keys_are_well_formed() {
        for k in GENERIC.iter().chain(KMS) {
            assert_eq!(k.len(), 29, "{k}");
            assert_eq!(k.matches('N').count(), 1, "{k}");
            assert!(!k.ends_with('N'), "{k}");
            assert!(
                k.bytes()
                    .all(|c| c == b'-' || c == b'N' || CHARS.contains(&c)),
                "{k}"
            );
        }
    }
}
