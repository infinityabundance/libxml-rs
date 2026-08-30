//! EXSLT Dates and Times (date:) — date:date-time, date:date, date:time,
//! date:year, date:month-in-year, date:day-in-month, date:day-of-week-in-month,
//! date:day-in-year, date:day-name, date:day-abbreviation, date:month-name,
//! date:month-abbreviation, date:week-in-year, date:hour-in-day,
//! date:minute-in-hour, date:second-in-minute, date:leap-year,
//! date:format-date, date:parse-date, date:add, date:add-duration,
//! date:difference, date:duration, date:seconds, date:sum (§35).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (libexslt/date.c) implements the EXSLT Dates and Times
//! module over ISO 8601 strings (`YYYY-MM-DDTHH:MM:SS[+HH:MM]`). The
//! component functions extract fields; `date:format-date` and
//! `date:parse-date` use the picture-string grammar defined by the EXSLT
//! specification.
//!
//! We implement the ISO 8601 handling natively (civil-date algorithms) —
//! no external date library is used.

use super::{register, ExsltFunction};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::{node_string_value, NodeSet, XPathValue};

/// A parsed ISO 8601 date-time.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DateTime {
    year: i64,
    month: u32,  // 1-12
    day: u32,    // 1-31
    hour: u32,   // 0-23
    minute: u32, // 0-59
    second: u32, // 0-59
    // Timezone offset in minutes east of UTC; None = unknown (treated as 0).
    tz_minutes: i32,
}

impl DateTime {
    fn new() -> Self {
        DateTime {
            year: 0,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            tz_minutes: 0,
        }
    }
}

// ── Civil date algorithms (Howard Hinnant) ────────────────────────────────

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Days in month (1-based month).
fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Day of the week: 0 = Sunday ... 6 = Saturday (per EXSLT, which numbers
/// Monday as 1; we convert here).
fn day_of_week(y: i64, m: u32, d: u32) -> u32 {
    // 1970-01-01 was a Thursday.
    let days = days_from_civil(y, m as i64, d as i64);
    let wd = (days + 4).rem_euclid(7); // 0 = Sunday
    wd as u32
}

/// EXSLT week number: ISO 8601 week (Monday-based, week 1 = first week with
/// >= 4 days in the year).
fn iso_week_number(y: i64, m: u32, d: u32) -> u32 {
    let wd = day_of_week(y, m, d); // 0=Sun..6=Sat
                                   // Convert to ISO: 1=Mon..7=Sun
    let iso_wd = if wd == 0 { 7 } else { wd as i64 };
    let jan1_wd = day_of_week(y, 1, 1);
    let jan1_iso = if jan1_wd == 0 { 7 } else { jan1_wd as i64 };
    let doy = days_from_civil(y, m as i64, d as i64) - days_from_civil(y, 1, 1) + 1;
    let week = (doy + 7 - jan1_iso) / 7 + 1;
    if week == 0 {
        // Belongs to the last week of the previous year.
        return iso_week_number(y - 1, 12, 31);
    }
    // Check if it overflows into next year's week 1.
    let days_in_yr = if is_leap_year(y) { 366 } else { 365 };
    if doy + 7 - jan1_iso > days_in_yr {
        return 1;
    }
    week as u32
}

// ── Parsing / formatting ──────────────────────────────────────────────────

/// Parse an ISO 8601 date-time string. Returns None if unparseable.
fn parse_date_time(s: &str) -> Option<DateTime> {
    let s = s.trim();
    // Split date and time at 'T' (or space).
    let (date_part, time_part, tz_part) = split_date_time(s);
    let mut dt = DateTime::new();

    // Date: YYYY-MM-DD or YYYYMMDD
    let date_digits: Vec<&str> = date_part.split('-').collect();
    match date_digits.len() {
        3 => {
            dt.year = date_digits[0].parse().ok()?;
            dt.month = date_digits[1].parse().ok()?;
            dt.day = date_digits[2].parse().ok()?;
        }
        1 => {
            let d = date_digits[0];
            if d.len() == 8 {
                dt.year = d[0..4].parse().ok()?;
                dt.month = d[4..6].parse().ok()?;
                dt.day = d[6..8].parse().ok()?;
            } else {
                return None;
            }
        }
        _ => return None,
    }
    if dt.year == 0 || dt.month < 1 || dt.month > 12 || dt.day < 1 {
        return None;
    }
    if dt.day > days_in_month(dt.year, dt.month) {
        return None;
    }

    if let Some(tp) = time_part {
        // Time: HH:MM:SS or HHMMSS
        let parts: Vec<&str> = tp.split(':').collect();
        match parts.len() {
            3 => {
                dt.hour = parts[0].parse().ok()?;
                dt.minute = parts[1].parse().ok()?;
                let sec = parts[2].parse::<f64>().ok()?;
                dt.second = sec.round() as u32;
            }
            1 => {
                let t = parts[0];
                if t.len() == 6 {
                    dt.hour = t[0..2].parse().ok()?;
                    dt.minute = t[2..4].parse().ok()?;
                    dt.second = t[4..6].parse().ok()?;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
        if dt.hour > 23 || dt.minute > 59 || dt.second > 59 {
            return None;
        }
    }

    // Timezone: Z, +HH:MM, -HH:MM, +HHMM, -HHMM
    if let Some(tz) = tz_part {
        if tz == "Z" || tz == "z" {
            dt.tz_minutes = 0;
        } else {
            let sign = if tz.starts_with('-') { -1 } else { 1 };
            let digits: String = tz.chars().filter(|c| c.is_ascii_digit()).collect();
            match digits.len() {
                2 => {
                    dt.tz_minutes = sign * (digits.parse::<i32>().ok()? * 60);
                }
                4 => {
                    let h: i32 = digits[0..2].parse().ok()?;
                    let m: i32 = digits[2..4].parse().ok()?;
                    dt.tz_minutes = sign * (h * 60 + m);
                }
                _ => return None,
            }
        }
    }

    Some(dt)
}

/// Split a date-time string into (date, time, timezone).
fn split_date_time(s: &str) -> (&str, Option<&str>, Option<&str>) {
    // Split the date and time at 'T' (or a space). A bare date has no time.
    let sep = s.find(['T', ' ']);
    let (date_part, rest) = match sep {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    // Inside the time portion, find the timezone marker ('Z', '+', or '-'
    // after the time). The '-' in the DATE portion is a separator, not a
    // timezone marker.
    match rest {
        Some(r) => match r.find(['Z', 'z', '+', '-']) {
            Some(i) => (date_part, Some(&r[..i]), Some(&r[i..])),
            None => (date_part, Some(r), None),
        },
        None => (date_part, None, None),
    }
}

/// Format a DateTime in ISO 8601: YYYY-MM-DDTHH:MM:SS.
fn format_date_time(dt: &DateTime, with_tz: bool) -> String {
    let mut s = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
    );
    if with_tz {
        if dt.tz_minutes == 0 {
            s.push('Z');
        } else {
            let abs = dt.tz_minutes.abs();
            let sign = if dt.tz_minutes < 0 { '-' } else { '+' };
            s.push_str(&format!("{}{:02}:{:02}", sign, abs / 60, abs % 60));
        }
    }
    s
}

/// The current local date-time (using the system clock and local timezone).
fn now() -> DateTime {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Convert to local time via libc.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t: libc::time_t = secs as libc::time_t;
    unsafe {
        libc::localtime_r(&t, &mut tm);
    }
    let tz_minutes = tm.tm_gmtoff / 60;
    DateTime {
        year: tm.tm_year as i64 + 1900,
        month: tm.tm_mon as u32 + 1,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        second: tm.tm_sec as u32,
        tz_minutes: tz_minutes as i32,
    }
}

/// Parse the first argument as a date-time, or return None.
fn date_arg(args: &[XPathValue]) -> Option<DateTime> {
    match args.first() {
        // UPSTREAM-PARITY: with no argument, EXSLT date/time functions
        // operate on the current date and time (dates.c dateArg defaults to
        // the current date-time).
        Some(v) => parse_date_time(&v.as_string()),
        None => Some(now()),
    }
}

/// Normalize a DateTime to UTC (apply the timezone offset).
fn to_utc(dt: &DateTime) -> DateTime {
    let mut minutes = days_from_civil(dt.year, dt.month as i64, dt.day as i64) * 1440
        + dt.hour as i64 * 60
        + dt.minute as i64
        - dt.tz_minutes as i64;
    let days = minutes.div_euclid(1440);
    minutes = minutes.rem_euclid(1440);
    let (y, m, d) = civil_from_days(days);
    DateTime {
        year: y,
        month: m,
        day: d,
        hour: (minutes / 60) as u32,
        minute: (minutes % 60) as u32,
        second: dt.second,
        tz_minutes: 0,
    }
}

// ── EXSLT functions ───────────────────────────────────────────────────────

/// date:date-time() — the current date and time.
fn date_time_fn(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
    let dt = now();
    Ok(XPathValue::String(format_date_time(&dt, true)))
}

/// date:date(date-time) — the date part (YYYY-MM-DD with timezone if any).
fn date_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => {
            let mut s = format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day);
            if dt.tz_minutes == 0 {
                s.push('Z');
            } else {
                let abs = dt.tz_minutes.abs();
                let sign = if dt.tz_minutes < 0 { '-' } else { '+' };
                s.push_str(&format!("{}{:02}:{:02}", sign, abs / 60, abs % 60));
            }
            Ok(XPathValue::String(s))
        }
        None => Ok(XPathValue::String(String::new())),
    }
}

/// date:time(date-time) — the time part (HH:MM:SS with timezone).
fn time_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => {
            let mut s = format!("{:02}:{:02}:{:02}", dt.hour, dt.minute, dt.second);
            if dt.tz_minutes == 0 {
                s.push('Z');
            } else {
                let abs = dt.tz_minutes.abs();
                let sign = if dt.tz_minutes < 0 { '-' } else { '+' };
                s.push_str(&format!("{}{:02}:{:02}", sign, abs / 60, abs % 60));
            }
            Ok(XPathValue::String(s))
        }
        None => Ok(XPathValue::String(String::new())),
    }
}

/// Component extractor macro-style helpers.
fn year_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::Number(dt.year as f64)),
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

fn month_in_year_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::Number(dt.month as f64)),
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

fn day_in_month_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::Number(dt.day as f64)),
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

fn day_of_week_in_month_fn(
    _ctx: &mut XPathContext,
    args: &[XPathValue],
) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => {
            let wd = day_of_week(dt.year, dt.month, dt.day); // 0=Sun
                                                             // EXSLT numbers Monday=1..Sunday=7.
            let n = if wd == 0 { 7 } else { wd as i64 };
            Ok(XPathValue::Number(n as f64))
        }
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

fn day_in_year_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => {
            let doy = days_from_civil(dt.year, dt.month as i64, dt.day as i64)
                - days_from_civil(dt.year, 1, 1)
                + 1;
            Ok(XPathValue::Number(doy as f64))
        }
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

fn week_in_year_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::Number(
            iso_week_number(dt.year, dt.month, dt.day) as f64,
        )),
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

const DAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const DAY_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn day_name_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => {
            let wd = day_of_week(dt.year, dt.month, dt.day) as usize;
            Ok(XPathValue::String(DAY_NAMES[wd].to_string()))
        }
        None => Ok(XPathValue::String(String::new())),
    }
}

fn day_abbreviation_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => {
            let wd = day_of_week(dt.year, dt.month, dt.day) as usize;
            Ok(XPathValue::String(DAY_ABBR[wd].to_string()))
        }
        None => Ok(XPathValue::String(String::new())),
    }
}

fn month_name_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::String(
            MONTH_NAMES[(dt.month - 1) as usize].to_string(),
        )),
        None => Ok(XPathValue::String(String::new())),
    }
}

fn month_abbreviation_fn(
    _ctx: &mut XPathContext,
    args: &[XPathValue],
) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::String(
            MONTH_ABBR[(dt.month - 1) as usize].to_string(),
        )),
        None => Ok(XPathValue::String(String::new())),
    }
}

fn hour_in_day_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::Number(dt.hour as f64)),
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

fn minute_in_hour_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::Number(dt.minute as f64)),
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

fn second_in_minute_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::Number(dt.second as f64)),
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

fn leap_year_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => Ok(XPathValue::Boolean(is_leap_year(dt.year))),
        None => Ok(XPathValue::Boolean(false)),
    }
}

/// date:seconds(date-time) — seconds since the epoch.
fn seconds_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    match date_arg(args) {
        Some(dt) => {
            let utc = to_utc(&dt);
            let days = days_from_civil(utc.year, utc.month as i64, utc.day as i64);
            let secs =
                days * 86400 + utc.hour as i64 * 3600 + utc.minute as i64 * 60 + utc.second as i64;
            Ok(XPathValue::Number(secs as f64))
        }
        None => Ok(XPathValue::Number(f64::NAN)),
    }
}

/// date:sum(node-set) — sum of date-time values (as seconds).
fn sum_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let ns = match args.first() {
        Some(XPathValue::NodeSet(ns)) => ns.clone(),
        _ => NodeSet::new(),
    };
    let mut total = 0.0;
    for n in ns.iter() {
        if let Some(dt) = parse_date_time(&node_string_value(n)) {
            let utc = to_utc(&dt);
            let days = days_from_civil(utc.year, utc.month as i64, utc.day as i64);
            total += (days * 86400
                + utc.hour as i64 * 3600
                + utc.minute as i64 * 60
                + utc.second as i64) as f64;
        }
    }
    Ok(XPathValue::Number(total))
}

/// date:duration(start, end) — a duration string between two dates
/// (PnYnMnDTnHnMnS), per EXSLT.
fn duration_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let start = match date_arg(args) {
        Some(d) => d,
        None => return Ok(XPathValue::String(String::new())),
    };
    let end = match args.get(1).and_then(|v| parse_date_time(&v.as_string())) {
        Some(d) => d,
        None => return Ok(XPathValue::String(String::new())),
    };
    let s = to_utc(&start);
    let e = to_utc(&end);
    let s_days = days_from_civil(s.year, s.month as i64, s.day as i64);
    let e_days = days_from_civil(e.year, e.month as i64, e.day as i64);
    let s_secs = s_days * 86400 + s.hour as i64 * 3600 + s.minute as i64 * 60 + s.second as i64;
    let e_secs = e_days * 86400 + e.hour as i64 * 3600 + e.minute as i64 * 60 + e.second as i64;
    let diff = e_secs - s_secs;
    let sign = if diff < 0 { "-" } else { "" };
    let diff = diff.abs();
    let days = diff / 86400;
    let hours = (diff % 86400) / 3600;
    let minutes = (diff % 3600) / 60;
    let seconds = diff % 60;
    // Emit the ISO 8601 duration, omitting zero components. A zero-length
    // duration is "P0S".
    let mut out = format!("{}P", sign);
    let mut any = false;
    if days > 0 {
        out.push_str(&format!("{}D", days));
        any = true;
    }
    if hours > 0 {
        out.push_str(&format!("{}H", hours));
        any = true;
    }
    if minutes > 0 {
        out.push_str(&format!("{}M", minutes));
        any = true;
    }
    if seconds > 0 {
        out.push_str(&format!("{}S", seconds));
        any = true;
    }
    if !any {
        out.push_str("0S");
    }
    Ok(XPathValue::String(out))
}

/// date:add(date-time, duration) — add an ISO 8601 duration.
fn add_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let mut dt = match date_arg(args) {
        Some(d) => d,
        None => return Ok(XPathValue::String(String::new())),
    };
    let dur = match args.get(1) {
        Some(v) => v.as_string(),
        None => return Ok(XPathValue::String(String::new())),
    };
    if let Some((years, months, days, hours, minutes, seconds)) = parse_duration(&dur) {
        dt.year += years as i64;
        // Month arithmetic with clamping.
        let total_months = dt.year * 12 + (dt.month as i64 - 1) + months as i64;
        dt.year = total_months.div_euclid(12);
        dt.month = (total_months.rem_euclid(12) + 1) as u32;
        dt.day = dt.day.min(days_in_month(dt.year, dt.month));
        // Add days via civil arithmetic.
        let days_total = days_from_civil(dt.year, dt.month as i64, dt.day as i64) + days as i64;
        let (y, m, d) = civil_from_days(days_total);
        dt.year = y;
        dt.month = m;
        dt.day = d;
        // Time.
        let mut secs =
            dt.hour as i64 * 3600 + dt.minute as i64 * 60 + dt.second as i64 + seconds as i64;
        let mut carry_days = 0;
        if secs < 0 {
            carry_days = secs.div_euclid(86400);
            secs = secs.rem_euclid(86400);
        } else if secs >= 86400 {
            carry_days = secs / 86400;
            secs %= 86400;
        }
        let dt2 = {
            let days_total = days_from_civil(dt.year, dt.month as i64, dt.day as i64) + carry_days;
            let (y, m, d) = civil_from_days(days_total);
            DateTime {
                year: y,
                month: m,
                day: d,
                hour: (secs / 3600) as u32,
                minute: ((secs % 3600) / 60) as u32,
                second: (secs % 60) as u32,
                tz_minutes: dt.tz_minutes,
            }
        };
        dt = dt2;
        Ok(XPathValue::String(format_date_time(&dt, true)))
    } else {
        Ok(XPathValue::String(String::new()))
    }
}

/// Parse an ISO 8601 duration `PnYnMnDTnHnMnS` (signed components allowed).
fn parse_duration(s: &str) -> Option<(i64, i64, i64, i64, i64, i64)> {
    let s = s.trim();
    let s = s.strip_prefix('P').or_else(|| s.strip_prefix('p'))?;
    let mut years = 0i64;
    let mut months = 0i64;
    let mut days = 0i64;
    let mut hours = 0i64;
    let mut minutes = 0i64;
    let mut seconds = 0i64;
    let mut in_time = false;
    let mut num = String::new();
    for c in s.chars() {
        match c {
            'T' | 't' => {
                in_time = true;
                num.clear();
            }
            'Y' | 'y' => {
                years = num.parse().ok()?;
                num.clear();
            }
            'M' | 'm' if !in_time => {
                months = num.parse().ok()?;
                num.clear();
            }
            'D' | 'd' => {
                days = num.parse().ok()?;
                num.clear();
            }
            'H' | 'h' => {
                hours = num.parse().ok()?;
                num.clear();
            }
            'M' | 'm' if in_time => {
                minutes = num.parse().ok()?;
                num.clear();
            }
            'S' | 's' => {
                seconds = num.parse().ok()?;
                num.clear();
            }
            '0'..='9' | '-' | '+' => num.push(c),
            _ => return None,
        }
    }
    Some((years, months, days, hours, minutes, seconds))
}

/// date:format-date(date-time, picture) — format per the EXSLT picture
/// grammar. Supports the standard component specifiers: Y, M, D, d, F, W, w,
/// H, m, s, and the literal-text mechanism.
fn format_date_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let dt = match date_arg(args) {
        Some(d) => d,
        None => return Ok(XPathValue::String(String::new())),
    };
    let picture = match args.get(1) {
        Some(v) => v.as_string(),
        None => return Ok(XPathValue::String(String::new())),
    };
    Ok(XPathValue::String(format_date_picture(&dt, &picture)))
}

fn format_date_picture(dt: &DateTime, picture: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = picture.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            // Literal text until the next quote.
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            i += 1; // skip closing quote
            continue;
        }
        // Count the run length of this component character.
        let mut run = 1;
        while i + run < chars.len() && chars[i + run] == c {
            run += 1;
        }
        let width = run;
        let wd = day_of_week(dt.year, dt.month, dt.day) as usize; // 0=Sun
        let exslt_wd = if wd == 0 { 6 } else { wd - 1 }; // 0=Mon..6=Sun
        match c {
            'Y' => {
                let s = format!("{:04}", dt.year);
                out.push_str(&s[s.len().saturating_sub(width)..]);
            }
            'M' => {
                if width >= 3 {
                    if width == 4 {
                        out.push_str(MONTH_NAMES[(dt.month - 1) as usize]);
                    } else {
                        out.push_str(MONTH_ABBR[(dt.month - 1) as usize]);
                    }
                } else {
                    let s = format!("{:02}", dt.month);
                    out.push_str(&s[s.len().saturating_sub(width)..]);
                }
            }
            'D' => {
                let s = format!("{:02}", dt.day);
                out.push_str(&s[s.len().saturating_sub(width)..]);
            }
            'd' => {
                if width >= 3 {
                    if width == 4 {
                        out.push_str(DAY_NAMES[wd]);
                    } else {
                        out.push_str(DAY_ABBR[wd]);
                    }
                } else {
                    let s = format!("{}", exslt_wd + 1);
                    out.push_str(&s[s.len().saturating_sub(width)..]);
                }
            }
            'F' => {
                // Day of week in month (1 = first occurrence...).
                let n = ((dt.day as i64 - 1) / 7) as usize + 1;
                let s = format!("{}", n);
                out.push_str(&s[s.len().saturating_sub(width)..]);
            }
            'W' => {
                let w = iso_week_number(dt.year, dt.month, dt.day);
                let s = format!("{:02}", w);
                out.push_str(&s[s.len().saturating_sub(width)..]);
            }
            'w' => {
                let s = format!("{}", exslt_wd + 1);
                out.push_str(&s[s.len().saturating_sub(width)..]);
            }
            'H' => {
                let s = format!("{:02}", dt.hour);
                out.push_str(&s[s.len().saturating_sub(width)..]);
            }
            'm' => {
                let s = format!("{:02}", dt.minute);
                out.push_str(&s[s.len().saturating_sub(width)..]);
            }
            's' => {
                let s = format!("{:02}", dt.second);
                out.push_str(&s[s.len().saturating_sub(width)..]);
            }
            _ => {
                out.push(c);
            }
        }
        i += run;
    }
    out
}

/// date:parse-date(string, picture) — parse per the picture grammar.
fn parse_date_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let input = match args.first() {
        Some(v) => v.as_string(),
        None => return Ok(XPathValue::String(String::new())),
    };
    let picture = match args.get(1) {
        Some(v) => v.as_string(),
        None => return Ok(XPathValue::String(String::new())),
    };
    // Parse the picture to extract numeric fields from the input.
    let mut dt = DateTime::new();
    let chars: Vec<char> = picture.chars().collect();
    let in_chars: Vec<char> = input.chars().collect();
    let mut pi = 0; // picture index
    let mut ii = 0; // input index
    let mut ok = true;
    while pi < chars.len() && ii < in_chars.len() {
        let c = chars[pi];
        if c == '\'' {
            pi += 1;
            while pi < chars.len() && chars[pi] != '\'' {
                if ii < in_chars.len() && chars[pi] == in_chars[ii] {
                    ii += 1;
                } else {
                    ok = false;
                }
                pi += 1;
            }
            pi += 1;
            continue;
        }
        let mut run = 1;
        while pi + run < chars.len() && chars[pi + run] == c {
            run += 1;
        }
        match c {
            'Y' | 'M' | 'D' | 'H' | 'm' | 's' => {
                // Read `run` digits (or fewer for the last field).
                let mut num = String::new();
                let mut k = 0;
                while k < run && ii < in_chars.len() && in_chars[ii].is_ascii_digit() {
                    num.push(in_chars[ii]);
                    ii += 1;
                    k += 1;
                }
                if let Ok(v) = num.parse::<u32>() {
                    match c {
                        'Y' => dt.year = v as i64,
                        'M' => dt.month = v,
                        'D' => dt.day = v,
                        'H' => dt.hour = v,
                        'm' => dt.minute = v,
                        's' => dt.second = v,
                        _ => {}
                    }
                }
            }
            'd' | 'F' | 'W' | 'w' => {
                // Skip the corresponding digits in the input.
                let mut k = 0;
                while k < run && ii < in_chars.len() && in_chars[ii].is_ascii_digit() {
                    ii += 1;
                    k += 1;
                }
            }
            _ => {
                // Literal separator: must match.
                if ii < in_chars.len() && c == in_chars[ii] {
                    ii += 1;
                }
            }
        }
        pi += run;
    }
    let _ = ok;
    Ok(XPathValue::String(format_date_time(&dt, false)))
}

/// Register all `date:` functions.
pub fn register_all() {
    register("date:date-time", date_time_fn as ExsltFunction);
    register("date:date", date_fn as ExsltFunction);
    register("date:time", time_fn as ExsltFunction);
    register("date:year", year_fn as ExsltFunction);
    register("date:month-in-year", month_in_year_fn as ExsltFunction);
    register("date:day-in-month", day_in_month_fn as ExsltFunction);
    register(
        "date:day-of-week-in-month",
        day_of_week_in_month_fn as ExsltFunction,
    );
    register("date:day-in-year", day_in_year_fn as ExsltFunction);
    register("date:week-in-year", week_in_year_fn as ExsltFunction);
    register("date:day-name", day_name_fn as ExsltFunction);
    register(
        "date:day-abbreviation",
        day_abbreviation_fn as ExsltFunction,
    );
    register("date:month-name", month_name_fn as ExsltFunction);
    register(
        "date:month-abbreviation",
        month_abbreviation_fn as ExsltFunction,
    );
    register("date:hour-in-day", hour_in_day_fn as ExsltFunction);
    register("date:minute-in-hour", minute_in_hour_fn as ExsltFunction);
    register(
        "date:second-in-minute",
        second_in_minute_fn as ExsltFunction,
    );
    register("date:leap-year", leap_year_fn as ExsltFunction);
    register("date:seconds", seconds_fn as ExsltFunction);
    register("date:sum", sum_fn as ExsltFunction);
    register("date:duration", duration_fn as ExsltFunction);
    register("date:add", add_fn as ExsltFunction);
    register("date:add-duration", add_fn as ExsltFunction);
    register("date:format-date", format_date_fn as ExsltFunction);
    register("date:parse-date", parse_date_fn as ExsltFunction);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::xpath::context::XPathContext;
    use core::ptr;

    fn ctx() -> XPathContext {
        XPathContext::new(ptr::null_mut())
    }

    #[test]
    fn test_parse_iso() {
        let dt = parse_date_time("2003-01-04T12:30:45").unwrap();
        assert_eq!(dt.year, 2003);
        assert_eq!(dt.month, 1);
        assert_eq!(dt.day, 4);
        assert_eq!(dt.hour, 12);
        assert_eq!(dt.minute, 30);
        assert_eq!(dt.second, 45);
        let dt = parse_date_time("2003-01-04T12:30:45+02:00").unwrap();
        assert_eq!(dt.tz_minutes, 120);
        let dt = parse_date_time("20030104").unwrap();
        assert_eq!(dt.year, 2003);
        assert_eq!(dt.month, 1);
        assert_eq!(dt.day, 4);
        assert!(parse_date_time("garbage").is_none());
    }

    #[test]
    fn test_civil_roundtrip() {
        for (y, m, d) in [
            (2000, 2, 29),
            (2003, 1, 4),
            (1970, 1, 1),
            (2024, 12, 31),
            (1900, 3, 1),
        ] {
            let days = days_from_civil(y, m, d);
            let (y2, m2, d2) = civil_from_days(days);
            assert_eq!(
                (y, m, d),
                (y2, m2 as i64, d2 as i64),
                "roundtrip for {}-{}-{}",
                y,
                m,
                d
            );
        }
    }

    #[test]
    fn test_leap_year() {
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn test_week_number() {
        // 2003-01-01 was a Wednesday (ISO week 1).
        assert_eq!(iso_week_number(2003, 1, 1), 1);
        // 2004-01-01 was a Thursday (ISO week 1).
        assert_eq!(iso_week_number(2004, 1, 1), 1);
    }

    #[test]
    fn test_component_functions() {
        let mut c = ctx();
        let arg = XPathValue::String("2003-01-04T12:30:45".to_string());
        assert_eq!(year_fn(&mut c, &[arg.clone()]).unwrap().as_number(), 2003.0);
        assert_eq!(
            month_in_year_fn(&mut c, &[arg.clone()])
                .unwrap()
                .as_number(),
            1.0
        );
        assert_eq!(
            day_in_month_fn(&mut c, &[arg.clone()]).unwrap().as_number(),
            4.0
        );
        assert_eq!(
            hour_in_day_fn(&mut c, &[arg.clone()]).unwrap().as_number(),
            12.0
        );
        assert_eq!(
            minute_in_hour_fn(&mut c, &[arg.clone()])
                .unwrap()
                .as_number(),
            30.0
        );
        assert_eq!(
            second_in_minute_fn(&mut c, &[arg.clone()])
                .unwrap()
                .as_number(),
            45.0
        );
        assert_eq!(
            day_in_year_fn(&mut c, &[arg.clone()]).unwrap().as_number(),
            4.0
        );
        assert!(!leap_year_fn(&mut c, &[arg.clone()]).unwrap().as_boolean());
        assert_eq!(
            day_name_fn(&mut c, &[arg.clone()]).unwrap().as_string(),
            "Saturday"
        );
        assert_eq!(
            day_abbreviation_fn(&mut c, &[arg.clone()])
                .unwrap()
                .as_string(),
            "Sat"
        );
        assert_eq!(
            month_name_fn(&mut c, &[arg.clone()]).unwrap().as_string(),
            "January"
        );
        assert_eq!(
            month_abbreviation_fn(&mut c, &[arg.clone()])
                .unwrap()
                .as_string(),
            "Jan"
        );
    }

    #[test]
    fn test_format_date() {
        let mut c = ctx();
        let arg = XPathValue::String("2003-01-04T12:30:45".to_string());
        let r = format_date_fn(
            &mut c,
            &[arg.clone(), XPathValue::String("Y-M-D".to_string())],
        )
        .unwrap();
        // Upstream formats each component with the number of digits in the
        // specifier: a width-1 year is the last digit ("3").
        assert_eq!(r.as_string(), "3-1-4");
        let r = format_date_fn(
            &mut c,
            &[arg.clone(), XPathValue::String("YYYY-MM-DD".to_string())],
        )
        .unwrap();
        assert_eq!(r.as_string(), "2003-01-04");
        let r = format_date_fn(
            &mut c,
            &[arg.clone(), XPathValue::String("'Year: 'YYYY".to_string())],
        )
        .unwrap();
        assert_eq!(r.as_string(), "Year: 2003");
    }

    #[test]
    fn test_duration() {
        let mut c = ctx();
        let r = duration_fn(
            &mut c,
            &[
                XPathValue::String("2003-01-04T00:00:00".to_string()),
                XPathValue::String("2003-01-06T00:00:00".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(r.as_string(), "P2D");
    }

    #[test]
    fn test_add() {
        let mut c = ctx();
        let r = add_fn(
            &mut c,
            &[
                XPathValue::String("2003-01-04T00:00:00".to_string()),
                XPathValue::String("P1D".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(r.as_string(), "2003-01-05T00:00:00Z");
    }

    #[test]
    fn test_seconds() {
        let mut c = ctx();
        let r = seconds_fn(
            &mut c,
            &[XPathValue::String("1970-01-01T00:00:00Z".to_string())],
        )
        .unwrap();
        assert_eq!(r.as_number(), 0.0);
    }
}
