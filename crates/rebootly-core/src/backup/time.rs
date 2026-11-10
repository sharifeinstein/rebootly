//! Minimal UTC timestamp formatting (no date/time dependency needed).

use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn parts(t: SystemTime) -> (i64, u32, u32, u32, u32, u32, u32) {
    let (secs, nanos) = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.subsec_nanos()),
        Err(e) => {
            let d = e.duration();
            let n = d.subsec_nanos();
            if n == 0 {
                (-(d.as_secs() as i64), 0)
            } else {
                (-(d.as_secs() as i64) - 1, 1_000_000_000 - n)
            }
        }
    };
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let sod = secs.rem_euclid(86_400) as u32;
    (y, mo, d, sod / 3600, sod / 60 % 60, sod % 60, nanos)
}

/// RFC 3339 in UTC, with nanoseconds only when non-zero.
pub fn rfc3339(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s, n) = parts(t);
    if n == 0 {
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
    } else {
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{n:09}Z")
    }
}

/// (year, month, day) → days since 1970-01-01 (inverse of `civil_from_days`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse what [`rfc3339`] writes (`YYYY-MM-DDTHH:MM:SS[.fraction]Z`).
pub fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.splitn(3, '-').map(str::parse::<i64>);
    let (y, mo, da) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut t = hms.splitn(3, ':').map(str::parse::<u64>);
    let (h, mi, se) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let nanos = if frac.is_empty() {
        0
    } else {
        if frac.len() > 9 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        format!("{frac:0<9}").parse::<u32>().ok()?
    };
    let secs = days_from_civil(y, mo as u32, da as u32) * 86_400 + (h * 3600 + mi * 60 + se) as i64;
    let base = if secs >= 0 {
        UNIX_EPOCH + std::time::Duration::from_secs(secs as u64)
    } else {
        UNIX_EPOCH - std::time::Duration::from_secs(secs.unsigned_abs())
    };
    Some(base + std::time::Duration::from_nanos(u64::from(nanos)))
}

/// `YYYYMMDD-HHMMSS` for folder names.
pub fn stamp(t: SystemTime) -> String {
    let (y, mo, d, h, mi, s, _) = parts(t);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats() {
        // 500 ns: representable on Windows too (100 ns SystemTime ticks).
        let t = UNIX_EPOCH + Duration::new(1_794_249_361, 500);
        assert_eq!(rfc3339(t), "2026-11-09T18:36:01.000000500Z");
        assert_eq!(stamp(t), "20261109-183601");
        assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            rfc3339(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00Z"
        );
        assert_eq!(
            rfc3339(UNIX_EPOCH - Duration::from_millis(500)),
            "1969-12-31T23:59:59.500000000Z"
        );
    }

    #[test]
    fn parses_what_it_formats() {
        for t in [
            UNIX_EPOCH + Duration::new(1_794_249_361, 500),
            UNIX_EPOCH,
            UNIX_EPOCH + Duration::from_secs(951_782_400),
            UNIX_EPOCH - Duration::from_millis(500),
        ] {
            assert_eq!(parse_rfc3339(&rfc3339(t)), Some(t));
        }
        assert_eq!(
            parse_rfc3339("2026-11-09T22:29:35.5Z"),
            Some(UNIX_EPOCH + Duration::new(1_794_263_375, 500_000_000))
        );
        for bad in [
            "",
            "2026-11-09",
            "2026-13-01T00:00:00Z",
            "2026-11-09T22:29:35+01:00",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }
}
