//! Convert a Unix mtime (seconds since 1970) into a FAT dos_date/dos_time
//! pair, packed as `(dos_date << 16) | dos_time`. Truncates to FAT's
//! 2-second resolution. Out-of-range inputs are clamped to the
//! endpoint of the representable range: pre-1980 dates collapse to
//! 1980-01-01 00:00:00 (all components clamped, not just the year),
//! and post-2107 dates collapse to 2107-12-31 23:59:58. A year-only
//! clamp would corrupt the month/day/time fields (e.g. 1979-06-15
//! 17:42 → 1980-06-15 17:42) without the caller noticing.

const FAT_EPOCH_YEAR: i32 = 1980;
const FAT_MAX_YEAR: i32 = 2107;

/// 1980-01-01 00:00:00 UTC as Unix seconds. `civil_from_unix` of any
/// input ≤ this collapses to the FAT epoch endpoint, so we can compare
/// in Unix-second space and short-circuit BEFORE running
/// `civil_from_unix`, which lets even pathological values like
/// `i64::MIN` produce a correct result instead of overflowing the
/// intermediate `i32` year.
const FAT_EPOCH_UNIX: i64 = 315_532_800;

/// 2107-12-31 23:59:58 UTC as Unix seconds. The 2-second FAT
/// truncation means 23:59:59 and 23:59:58 share the same encoding;
/// we clamp at the even value so the output round-trips through
/// `dos_time` without modification.
const FAT_MAX_UNIX: i64 = 4_354_905_598;

pub fn unix_to_fat(secs_since_epoch: i64) -> u32 {
    // Endpoint check in Unix-second space first, BEFORE
    // civil_from_unix. The intermediate `i32` year would overflow
    // for inputs near `i64::MAX` / `i64::MIN` if we always ran the
    // full conversion. Comparing directly against the precomputed
    // FAT endpoint Unix seconds avoids that overflow entirely.
    let (y, mo, d, h, mi, s) = if secs_since_epoch < FAT_EPOCH_UNIX {
        (FAT_EPOCH_YEAR, 1, 1, 0, 0, 0)
    } else if secs_since_epoch > FAT_MAX_UNIX {
        // 23:59:58 because dos_time stores seconds/2 in a 5-bit
        // field (max encoded value 29 → 58 s); 60 s would roll over
        // into the next minute, not into the field's bit pattern.
        // Day 31 is valid for December.
        (FAT_MAX_YEAR, 12, 31, 23, 59, 58)
    } else {
        civil_from_unix(secs_since_epoch)
    };
    let dos_date: u16 = (((y - FAT_EPOCH_YEAR) as u16) << 9) | ((mo as u16) << 5) | (d as u16);
    let dos_time: u16 = ((h as u16) << 11) | ((mi as u16) << 5) | ((s as u16) / 2);
    ((dos_date as u32) << 16) | (dos_time as u32)
}

/// Howard Hinnant's civil-from-days, adapted. Returns (year, month, day, hour, min, sec).
fn civil_from_unix(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400) as u32;
    let h = secs_of_day / 3600;
    let mi = (secs_of_day / 60) % 60;
    let s = secs_of_day % 60;

    // days since 1970-01-01 -> civil
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_clamps_to_1980() {
        // 1970-01-01 -> clamped to 1980-01-01 00:00:00
        let v = unix_to_fat(0);
        let dos_date = (v >> 16) as u16;
        let dos_time = (v & 0xFFFF) as u16;
        // year offset from 1980 = 0 -> high bits zero
        assert_eq!(dos_date >> 9, 0);
        assert_eq!((dos_date >> 5) & 0xF, 1, "month=1");
        assert_eq!(dos_date & 0x1F, 1, "day=1");
        assert_eq!(dos_time, 0);
    }

    #[test]
    fn pre_1980_non_jan1_clamps_to_epoch_endpoint() {
        // 1979-06-15 17:42:00 UTC -> must clamp ALL components, not
        // just the year. A year-only clamp would produce
        // 1980-06-15 17:42:00 and silently corrupt the date.
        // Unix seconds = 298_316_520; see the calculation in the
        // timestamp DOSBox-X gate's commit message.
        let v = unix_to_fat(298_316_520);
        let dos_date = (v >> 16) as u16;
        let dos_time = (v & 0xFFFF) as u16;
        assert_eq!(dos_date >> 9, 0, "year offset");
        assert_eq!((dos_date >> 5) & 0xF, 1, "month=1");
        assert_eq!(dos_date & 0x1F, 1, "day=1");
        assert_eq!(dos_time, 0, "time=00:00:00");
    }

    #[test]
    fn far_future_clamps_to_2107_endpoint() {
        // Year 3000 -> clamped to 2107-12-31 23:59:58.
        // 3000-01-01 UTC = 32_503_680_000 seconds.
        let v = unix_to_fat(32_503_680_000);
        let dos_date = (v >> 16) as u16;
        let dos_time = (v & 0xFFFF) as u16;
        assert_eq!(dos_date >> 9, (2107 - 1980) as u16);
        assert_eq!((dos_date >> 5) & 0xF, 12, "month=12");
        assert_eq!(dos_date & 0x1F, 31, "day=31");
        assert_eq!(dos_time >> 11, 23, "hour=23");
        assert_eq!((dos_time >> 5) & 0x3F, 59, "minute=59");
        assert_eq!((dos_time & 0x1F) * 2, 58, "second=58 (2s-truncated)");
    }

    #[test]
    fn pathological_inputs_clamp_without_overflow() {
        // Pre-fix, civil_from_unix(i64::MAX) overflowed the i32 year
        // before the endpoint clamp could run. Confirm both extremes
        // now collapse cleanly to the correct FAT endpoints.
        let max = unix_to_fat(i64::MAX);
        let max_date = (max >> 16) as u16;
        let max_time = (max & 0xFFFF) as u16;
        assert_eq!(max_date >> 9, (2107 - 1980) as u16, "i64::MAX year");
        assert_eq!((max_date >> 5) & 0xF, 12, "i64::MAX month=12");
        assert_eq!(max_date & 0x1F, 31, "i64::MAX day=31");
        assert_eq!(max_time >> 11, 23, "i64::MAX hour=23");

        let min = unix_to_fat(i64::MIN);
        let min_date = (min >> 16) as u16;
        let min_time = (min & 0xFFFF) as u16;
        assert_eq!(min_date >> 9, 0, "i64::MIN year offset = 0");
        assert_eq!((min_date >> 5) & 0xF, 1, "i64::MIN month=1");
        assert_eq!(min_date & 0x1F, 1, "i64::MIN day=1");
        assert_eq!(min_time, 0, "i64::MIN time=00:00:00");
    }

    #[test]
    fn known_timestamp() {
        // 2024-05-16 12:34:56 UTC = 1715862896
        let v = unix_to_fat(1_715_862_896);
        let dos_date = (v >> 16) as u16;
        let dos_time = (v & 0xFFFF) as u16;
        assert_eq!(dos_date >> 9, 2024 - 1980);
        assert_eq!((dos_date >> 5) & 0xF, 5);
        assert_eq!(dos_date & 0x1F, 16);
        assert_eq!(dos_time >> 11, 12);
        assert_eq!((dos_time >> 5) & 0x3F, 34);
        // 56 / 2 = 28
        assert_eq!(dos_time & 0x1F, 28);
    }
}
