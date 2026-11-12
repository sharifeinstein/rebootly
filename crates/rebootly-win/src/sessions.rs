//! Other signed-in users, as a pre-flight note (platform-neutral; the
//! Win32 listing is `live::sessions`). On the laptop a disconnected session
//! kept Edge running and 334 of its files couldn't be read.

/// A Windows session of someone other than the user running Rebootly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherSession {
    pub user: String,
    /// Switched away from or disconnected (apps keep running).
    pub disconnected: bool,
}

/// The note to show before a live backup, if anyone else is signed in.
/// One name per user (a user can have several sessions), sorted.
pub fn note(others: &[OtherSession]) -> Option<String> {
    let mut names: Vec<(String, bool)> = Vec::new();
    for s in others.iter().filter(|s| !s.user.trim().is_empty()) {
        match names
            .iter_mut()
            .find(|(n, _)| n.eq_ignore_ascii_case(&s.user))
        {
            Some((_, d)) => *d &= s.disconnected,
            None => names.push((s.user.clone(), s.disconnected)),
        }
    }
    if names.is_empty() {
        return None;
    }
    names.sort_by_key(|(n, _)| n.to_lowercase());
    let list: Vec<String> = names
        .iter()
        .map(|(n, d)| {
            if *d {
                format!("{n} (switched away, apps still running)")
            } else {
                n.clone()
            }
        })
        .collect();
    let who = if names.len() == 1 {
        "Another user is"
    } else {
        "Other users are"
    };
    Some(format!(
        "{who} still signed in on this PC: {}. Their open apps (browsers, mail) keep \
         files locked, and some can't be read even from a shadow copy. Sign them out \
         first: Task Manager > Users > right-click > Sign off.",
        list.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(user: &str, disconnected: bool) -> OtherSession {
        OtherSession {
            user: user.into(),
            disconnected,
        }
    }

    #[test]
    fn nobody_else_no_note() {
        assert_eq!(note(&[]), None);
        assert_eq!(note(&[s("", true), s("  ", false)]), None);
    }

    #[test]
    fn names_each_user_once() {
        let n = note(&[s("Amouni", true), s("bob", false), s("AMOUNI", true)]).unwrap();
        assert!(
            n.starts_with(
                "Other users are still signed in on this PC: \
                 Amouni (switched away, apps still running), bob."
            ),
            "{n}"
        );
        assert!(
            n.contains("Sign them out first: Task Manager > Users"),
            "{n}"
        );
        let one = note(&[s("Amouni", false), s("amouni", true)]).unwrap();
        assert!(
            one.starts_with("Another user is still signed in on this PC: Amouni."),
            "{one}"
        );
    }
}
