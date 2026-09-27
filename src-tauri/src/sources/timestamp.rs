//! 帖子发布时间的解析。两个站点的格式不同：Danbooru 是 ISO 8601（`2026-09-26T08:48:12.345-04:00`），
//! Gelbooru 是 `Sat Sep 27 01:02:03 -0500 2026`。统一换算成 Unix 毫秒存进图库，按上传先后排序时用。

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// 解析成 Unix 毫秒；认不出的格式返回 `None`。
pub fn parse(value: &str) -> Option<i64> {
    let value = value.trim();
    parse_iso(value).or_else(|| parse_ctime(value))
}

/// `2026-09-26T08:48:12.345-04:00`。小数秒和时区可以省略，省略时区按 UTC 算。
fn parse_iso(value: &str) -> Option<i64> {
    let (date, time) = value.split_once(['T', ' '])?;
    let mut date = date.split('-');
    let (year, month, day) = (date.next()?.parse().ok()?, date.next()?.parse().ok()?, date.next()?.parse().ok()?);
    if date.next().is_some() {
        return None;
    }
    let (hour, minute, second) = parse_clock(time.get(..8)?)?;
    let mut rest = &time[8..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.find(|c: char| !c.is_ascii_digit()).unwrap_or(fraction.len());
        if digits == 0 {
            return None;
        }
        // 只精确到毫秒：多的截掉，不足三位补零。
        let ms: String = fraction[..digits].chars().chain("000".chars()).take(3).collect();
        millis = ms.parse().ok()?;
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        _ => parse_offset(rest)?,
    };
    to_millis(year, month, day, (hour, minute, second), millis, offset)
}

/// `Sat Sep 27 01:02:03 -0500 2026`
fn parse_ctime(value: &str) -> Option<i64> {
    let parts: Vec<&str> = value.split_whitespace().collect();
    let [_, month, day, clock, offset, year] = parts.as_slice() else { return None };
    let month = MONTHS.iter().position(|m| m.eq_ignore_ascii_case(month))? as u32 + 1;
    to_millis(year.parse().ok()?, month, day.parse().ok()?, parse_clock(clock)?, 0, parse_offset(offset)?)
}

/// `HH:MM:SS`
fn parse_clock(value: &str) -> Option<(u32, u32, u32)> {
    let mut parts = value.split(':');
    let clock = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    parts.next().is_none().then_some(clock)
}

/// `+09:00` 或 `-0500`，返回比 UTC 快多少分钟。
fn parse_offset(value: &str) -> Option<i64> {
    let sign = match value.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: String = value[1..].chars().filter(|c| *c != ':').collect();
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (hours, minutes): (i64, i64) = (digits[..2].parse().ok()?, digits[2..].parse().ok()?);
    (hours < 24 && minutes < 60).then_some(sign * (hours * 60 + minutes))
}

fn to_millis(year: i64, month: u32, day: u32, clock: (u32, u32, u32), millis: i64, offset: i64) -> Option<i64> {
    let (hour, minute, second) = clock;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + i64::from(hour * 3600 + minute * 60 + second);
    Some((seconds - offset * 60) * 1000 + millis)
}

/// 公历日期距 1970-01-01 的天数（Howard Hinnant 的 days_from_civil）。
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((i64::from(month) + 9) % 12) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_sites() {
        assert_eq!(parse("2026-09-26T08:48:12.345-04:00"), Some(1_790_426_892_345));
        assert_eq!(parse("Sat Sep 27 01:02:03 -0500 2026"), Some(1_790_488_923_000));
        assert_eq!(parse("2007-02-03T04:05:06Z"), Some(1_170_475_506_000));
        assert_eq!(parse("1999-12-31T23:59:59+09:00"), Some(946_652_399_000));
        assert_eq!(parse("Mon Feb 29 12:00:00 +0000 2016"), Some(1_456_747_200_000));
        assert_eq!(parse("2007-02-03T04:05:06.123456Z"), Some(1_170_475_506_123));
        assert_eq!(parse("2007-02-03T04:05:06.5Z"), Some(1_170_475_506_500));
    }

    #[test]
    fn rejects_unknown_formats() {
        for value in ["", "yesterday", "2026-09-26", "2026-13-01T00:00:00Z", "2026-09-26T08:48:12+5", "Sat Foo 27 01:02:03 -0500 2026"] {
            assert_eq!(parse(value), None, "{value}");
        }
    }
}
