//! BitLocker recovery keys (SPEC §7.1): 48 digits in 8 groups of 6. Each
//! group is a multiple of 11 below 720 896 (11 × 65 536), which catches
//! most typos before the unlock tool is even run.
//!
//! The key is held as a `SecretString` and never shown, logged or put in an
//! error message.

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
        for (i, g) in groups.iter().enumerate() {
            let n: u32 = g.parse().map_err(|_| KeyError::Group(i + 1))?;
            if n % 11 != 0 || n / 11 >= 65_536 {
                return Err(KeyError::Group(i + 1));
            }
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
    fn debug_is_redacted() {
        let k = RecoveryKey::parse(KEY).unwrap();
        assert_eq!(format!("{k:?}"), "RecoveryKey(…)");
    }
}
