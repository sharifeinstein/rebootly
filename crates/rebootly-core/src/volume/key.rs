//! BitLocker recovery keys (SPEC §7.1): 48 digits in 8 groups of 6. Each
//! group is a multiple of 11 below 720 896 (11 × 65 536), which catches
//! most typos before the unlock tool is even run.
//!
//! The key is shown only while it's typed (grouped, like Windows' own
//! recovery screen, so it can be checked against the printout; owner
//! decision 2026-11-11). Once entered it's held as a `SecretString` and
//! never shown, logged or put in an error message.

use age::secrecy::{ExposeSecret, SecretString};

pub struct RecoveryKey(SecretString);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    #[error("a recovery key has only digits and dashes")]
    BadCharacter,
    #[error("a recovery key has 48 digits (8 groups of 6); this has {0}")]
    Length(usize),
    #[error("group {0} of the recovery key isn't valid: check it for a typo")]
    Group(usize),
}

fn group_ok(g: &str) -> bool {
    g.parse::<u32>().is_ok_and(|n| n % 11 == 0 && n / 11 < 65_536)
}

/// A key as it's being typed, shown back: its digits in groups of 6
/// (`123456-234567-12`), at most 48; anything but digits is dropped.
pub fn grouped(input: &str) -> String {
    let mut out = String::with_capacity(55);
    for (i, c) in input.chars().filter(char::is_ascii_digit).take(48).enumerate() {
        if i > 0 && i % 6 == 0 {
            out.push('-');
        }
        out.push(c);
    }
    out
}

/// The 48 digits of a whole key at byte `start`: 8 groups of 6 digits,
/// each but the first optionally after one dash or space, and no digit
/// right after.
fn key_at(b: &[u8], start: usize) -> Option<String> {
    let mut d = String::with_capacity(48);
    let mut j = start;
    for g in 0..8 {
        if g > 0 && matches!(b.get(j), Some(b'-' | b' ')) {
            j += 1;
        }
        for _ in 0..6 {
            let c = *b.get(j).filter(|c| c.is_ascii_digit())?;
            d.push(c as char);
            j += 1;
        }
    }
    (!b.get(j).is_some_and(u8::is_ascii_digit)).then_some(d)
}

/// The first whole key in a text, grouped: e.g. a line copied from
/// aka.ms/myrecoverykey, which also holds the key ID and a date.
pub fn find_key(text: &str) -> Option<String> {
    let b = text.as_bytes();
    (0..b.len())
        .filter(|&i| b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit()))
        .find_map(|i| key_at(b, i))
        .map(|d| grouped(&d))
}

/// A pasted text as key digits: the key found in it, or its digits if it
/// holds nothing else but dashes and spaces (part of a key). `None` if it
/// isn't a key (so other digits don't creep in).
pub fn pasted_digits(text: &str) -> Option<String> {
    if let Some(k) = find_key(text) {
        return Some(k.replace('-', ""));
    }
    let t = text.trim();
    (!t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | ' ')))
        .then(|| t.chars().filter(char::is_ascii_digit).collect())
}

/// What a key field shows after an edit or a paste: a whole key found in
/// it, else its digits grouped.
pub fn tidy(text: &str) -> String {
    find_key(text).unwrap_or_else(|| grouped(text))
}

/// Digits typed so far, and the first finished group (1-based) that can't
/// be right.
pub fn progress(input: &str) -> (usize, Option<usize>) {
    let digits: String = input.chars().filter(char::is_ascii_digit).collect();
    let n = digits.len();
    let bad = (0..n.min(48) / 6).find(|&i| !group_ok(&digits[i * 6..i * 6 + 6]));
    (n, bad.map(|i| i + 1))
}

/// One short line about a key being typed: how many digits so far, or the
/// first finished group that can't be right (a typo shows up at once).
pub fn typing_status(input: &str) -> String {
    let (n, bad) = progress(input);
    match bad {
        Some(g) => format!("{n} of 48 digits; group {g} isn't right, check it"),
        None if n == 48 => "48 of 48 digits".to_string(),
        None if n > 48 => format!("{n} of 48 digits: too many"),
        None => format!("{n} of 48 digits"),
    }
}

impl RecoveryKey {
    /// Accepts `111111-222222-…`, spaces instead of dashes, or 48 digits.
    pub fn parse(input: &str) -> Result<Self, KeyError> {
        let mut digits = String::with_capacity(48);
        for c in input.trim().chars() {
            match c {
                '0'..='9' => digits.push(c),
                '-' | ' ' => {}
                _ => return Err(KeyError::BadCharacter),
            }
        }
        if digits.len() != 48 {
            return Err(KeyError::Length(digits.len()));
        }
        let groups: Vec<&str> = (0..8).map(|i| &digits[i * 6..i * 6 + 6]).collect();
        if let Some(i) = groups.iter().position(|g| !group_ok(g)) {
            return Err(KeyError::Group(i + 1));
        }
        Ok(RecoveryKey(SecretString::from(groups.join("-"))))
    }

    /// The key in the `111111-222222-…` form the unlock tools expect.
    /// Only for passing to `manage-bde` / `cryptsetup`; never print it.
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl std::fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryKey(…)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 8 valid groups: multiples of 11 below 720 896.
    const KEY: &str = "000000-000011-123453-720885-111111-222222-333333-444444";

    #[test]
    fn accepts_dashes_spaces_and_plain_digits() {
        let plain: String = KEY.chars().filter(|c| *c != '-').collect();
        let spaced = KEY.replace('-', " ");
        for s in [KEY, &plain, &spaced, &format!("  {KEY}\n")] {
            assert_eq!(RecoveryKey::parse(s).unwrap().expose(), KEY);
        }
    }

    #[test]
    fn errors_name_the_problem_but_never_the_key() {
        assert_eq!(
            RecoveryKey::parse("12345").unwrap_err(),
            KeyError::Length(5)
        );
        assert_eq!(
            RecoveryKey::parse(&KEY.replace('-', "/")).unwrap_err(),
            KeyError::BadCharacter
        );
        // 123454 is not a multiple of 11 (group 3); 720896 = 11 × 65536.
        let typo = KEY.replace("123453", "123454");
        let e = RecoveryKey::parse(&typo).unwrap_err();
        assert_eq!(e, KeyError::Group(3));
        assert!(!e.to_string().contains("123454"));
        let big = KEY.replace("720885", "720896");
        assert_eq!(RecoveryKey::parse(&big).unwrap_err(), KeyError::Group(4));
    }

    #[test]
    fn grouped_while_typing() {
        assert_eq!(grouped(""), "");
        assert_eq!(grouped("123456"), "123456");
        assert_eq!(grouped("1234567"), "123456-7");
        assert_eq!(grouped("123456-7"), "123456-7");
        assert_eq!(grouped("123 456 78x9"), "123456-789");
        assert_eq!(grouped(&format!("{KEY}99")), KEY);
    }

    #[test]
    fn typing_status_counts_and_names_a_bad_group() {
        assert_eq!(typing_status(""), "0 of 48 digits");
        assert_eq!(typing_status("00001"), "5 of 48 digits");
        assert_eq!(typing_status(KEY), "48 of 48 digits");
        let typo = KEY.replace("123453", "123454");
        assert_eq!(
            typing_status(&typo[..20]),
            "18 of 48 digits; group 3 isn't right, check it"
        );
        assert!(typing_status(&format!("{KEY}1")).contains("too many"));
    }

    #[test]
    fn finds_a_key_in_pasted_text() {
        let page = format!("Key ID: 1A2B3C4D\tRecovery key: {KEY}\tUploaded 11/11/2026");
        assert_eq!(find_key(&page).as_deref(), Some(KEY));
        assert_eq!(find_key(&KEY.replace('-', " ")).as_deref(), Some(KEY));
        let plain: String = KEY.chars().filter(char::is_ascii_digit).collect();
        assert_eq!(find_key(&plain).as_deref(), Some(KEY));
        // Not 6-digit groups, or too long.
        assert_eq!(find_key("2026-11-11 123456"), None);
        assert_eq!(find_key(&format!("{plain}0")), None);
        assert_eq!(pasted_digits(&page), Some(plain.clone()));
        assert_eq!(pasted_digits(" 123456-23 "), Some("12345623".into()));
        assert_eq!(pasted_digits("Key ID 1A2B, 11/11/2026"), None);
        assert_eq!(tidy(&format!("000000-{page}")), KEY);
        assert_eq!(tidy("1234567"), "123456-7");
    }

    #[test]
    fn debug_is_redacted() {
        let k = RecoveryKey::parse(KEY).unwrap();
        assert_eq!(format!("{k:?}"), "RecoveryKey(…)");
    }
}
