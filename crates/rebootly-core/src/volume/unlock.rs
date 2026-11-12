//! What opens a locked BitLocker volume (SPEC §7.1): its 48-digit recovery
//! key, or the password set for it (data drives, and OS drives on PCs
//! without a TPM). A TPM PIN can't: the TPM releases the key only to the
//! original PC's own Windows boot, never to WinPE, Linux or another PC.
//!
//! Both are held as `SecretString`s; `Debug` is redacted and nothing here
//! puts them in a message.

use age::secrecy::{ExposeSecret, SecretString};

use super::key::RecoveryKey;

/// A BitLocker password (Windows allows 8–256 characters; only the upper
/// bound and "not empty" are checked, policy can lower the minimum).
pub struct Password(SecretString);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    #[error("the password is empty")]
    Empty,
    #[error("a BitLocker password has at most 256 characters")]
    TooLong,
}

impl Password {
    /// Takes the text as is, except a trailing line break (from a file or
    /// a pipe): spaces can be part of a password.
    pub fn new(input: &str) -> Result<Self, PasswordError> {
        let p = input.trim_end_matches(['\r', '\n']);
        match p.chars().count() {
            0 => Err(PasswordError::Empty),
            n if n > 256 => Err(PasswordError::TooLong),
            _ => Ok(Password(SecretString::from(p.to_string()))),
        }
    }

    /// Only for passing to the unlock tool; never print it.
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(…)")
    }
}

/// A recovery key or a password.
#[derive(Debug)]
pub enum Unlock {
    Key(RecoveryKey),
    Password(Password),
}

impl Unlock {
    /// The secret itself, for the unlock tool only.
    pub fn expose(&self) -> &str {
        match self {
            Unlock::Key(k) => k.expose(),
            Unlock::Password(p) => p.expose(),
        }
    }

    pub fn is_password(&self) -> bool {
        matches!(self, Unlock::Password(_))
    }

    /// "recovery key" or "password", for messages.
    pub fn what(&self) -> &'static str {
        if self.is_password() {
            "password"
        } else {
            "recovery key"
        }
    }
}

impl From<RecoveryKey> for Unlock {
    fn from(k: RecoveryKey) -> Self {
        Unlock::Key(k)
    }
}

impl From<Password> for Unlock {
    fn from(p: Password) -> Self {
        Unlock::Password(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_keeps_spaces_drops_the_line_break() {
        assert_eq!(Password::new(" a b \r\n").unwrap().expose(), " a b ");
        assert_eq!(Password::new("\n").unwrap_err(), PasswordError::Empty);
        assert_eq!(
            Password::new(&"x".repeat(257)).unwrap_err(),
            PasswordError::TooLong
        );
        assert!(Password::new(&"é".repeat(256)).is_ok());
    }

    #[test]
    fn unlock_is_redacted_and_says_what_it_is() {
        let p: Unlock = Password::new("hunter2!").unwrap().into();
        assert!(p.is_password());
        assert_eq!(p.what(), "password");
        assert_eq!(p.expose(), "hunter2!");
        assert_eq!(format!("{p:?}"), "Password(Password(…))");
        let k: Unlock =
            RecoveryKey::parse("000000-000011-123453-720885-111111-222222-333333-444444")
                .unwrap()
                .into();
        assert_eq!(k.what(), "recovery key");
        assert!(!format!("{k:?}").contains("123453"));
    }
}
