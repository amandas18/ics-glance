//! Minimal iCalendar (RFC 5545) reading support.
//!
//! This is not a full implementation of the spec. It handles the parts
//! that show up in real .ics files often enough to matter: folded lines,
//! CRLF/LF mixed line endings, quoted parameter values, and the standard
//! backslash escapes inside property values.

use std::fmt;

/// A single unfolded, parsed content line: `NAME;PARAM=VAL;...:VALUE`.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

/// A calendar date (Gregorian), with no time or timezone attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    fn parse(s: &str) -> Option<Date> {
        if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let year: i32 = s[0..4].parse().ok()?;
        let month: u32 = s[4..6].parse().ok()?;
        let day: u32 = s[6..8].parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(Date { year, month, day })
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// A time of day, with no timezone attached (see `TimeKind` for that).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Time {
    fn parse(s: &str) -> Option<Time> {
        if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let hour: u32 = s[0..2].parse().ok()?;
        let minute: u32 = s[2..4].parse().ok()?;
        let second: u32 = s[4..6].parse().ok()?;
        // second allows 60 for a leap second, per RFC 5545 3.3.12.
        if hour > 23 || minute > 59 || second > 60 {
            return None;
        }
        Some(Time { hour, minute, second })
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

/// How a date-time's timezone was specified in the source property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeKind {
    /// No `Z` suffix and no `TZID`: local to whatever reads it back.
    Floating,
    /// Trailing `Z` on the value.
    Utc,
    /// `TZID` parameter naming a timezone; not resolved to an offset here.
    Zone(String),
}

/// A parsed `DTSTART`/`DTEND` value: either a bare date (`VALUE=DATE`)
/// or a date paired with a time and its timezone kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateTimeValue {
    Date(Date),
    DateTime {
        date: Date,
        time: Time,
        kind: TimeKind,
    },
}

impl fmt::Display for DateTimeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DateTimeValue::Date(d) => write!(f, "{d}"),
            DateTimeValue::DateTime { date, time, kind } => match kind {
                TimeKind::Floating => write!(f, "{date} {time}"),
                TimeKind::Utc => write!(f, "{date} {time} UTC"),
                TimeKind::Zone(tz) => write!(f, "{date} {time} {tz}"),
            },
        }
    }
}

/// Parses the date/date-time shape shared by DTSTART, DTEND, and an
/// RRULE's UNTIL value, without any parameter handling. UNTIL never
/// carries its own TZID - RFC 5545 requires it to match DTSTART's value
/// type, using UTC rather than a zone for date-times.
fn parse_date_time_value(value: &str) -> Option<DateTimeValue> {
    if !value.contains('T') {
        return Some(DateTimeValue::Date(Date::parse(value)?));
    }

    let mut parts = value.splitn(2, 'T');
    let date_part = parts.next()?;
    let mut time_part = parts.next()?;

    let is_utc = time_part.ends_with('Z');
    if is_utc {
        time_part = &time_part[..time_part.len() - 1];
    }

    let date = Date::parse(date_part)?;
    let time = Time::parse(time_part)?;
    let kind = if is_utc { TimeKind::Utc } else { TimeKind::Floating };

    Some(DateTimeValue::DateTime { date, time, kind })
}

/// Parses a `DTSTART`/`DTEND`-shaped property into a `DateTimeValue`.
///
/// Honors an explicit `VALUE=DATE` parameter; failing that, the shape of
/// the value decides (no `T` means a bare date). A trailing `Z` on the
/// value means UTC; otherwise a `TZID` parameter names the timezone;
/// otherwise the time is floating (RFC 5545 section 3.3.5).
pub fn parse_date_time(prop: &Property) -> Option<DateTimeValue> {
    let value_is_date = prop
        .params
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("VALUE") && v.eq_ignore_ascii_case("DATE"));

    if value_is_date {
        return Some(DateTimeValue::Date(Date::parse(&prop.value)?));
    }

    let mut result = parse_date_time_value(&prop.value)?;
    if let DateTimeValue::DateTime { kind, .. } = &mut result {
        if *kind == TimeKind::Floating {
            if let Some((_, tzid)) = prop
                .params
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("TZID"))
            {
                *kind = TimeKind::Zone(tzid.clone());
            }
        }
    }
    Some(result)
}

/// The `FREQ` of an `RRULE` (RFC 5545 section 3.3.10). Only the four base
/// frequencies are handled; sub-daily ones (`HOURLY`, `MINUTELY`,
/// `SECONDLY`) don't show up in real calendars often enough to bother with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// A parsed `RRULE` value covering `FREQ`, `INTERVAL`, `COUNT`, and
/// `UNTIL`. It does not understand `BYDAY`/`BYMONTHDAY`/etc selectors, so
/// a rule that depends on one of those (e.g. "every 2nd Tuesday") will
/// parse but `expand` won't produce the selective sequence it implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurrence {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<DateTimeValue>,
}

/// Parses an `RRULE` property value, e.g. `FREQ=WEEKLY;INTERVAL=2;COUNT=10`.
pub fn parse_rrule(value: &str) -> Option<Recurrence> {
    let mut freq: Option<Freq> = None;
    let mut interval: u32 = 1;
    let mut count: Option<u32> = None;
    let mut until: Option<DateTimeValue> = None;

    for part in value.split(';') {
        if part.is_empty() {
            continue;
        }
        let mut kv = part.splitn(2, '=');
        let key = kv.next()?;
        let val = kv.next()?;
        match key {
            "FREQ" => {
                freq = Some(match val {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None,
                });
            }
            "INTERVAL" => interval = val.parse().ok().filter(|n| *n > 0)?,
            "COUNT" => count = Some(val.parse().ok()?),
            "UNTIL" => until = Some(parse_date_time_value(val)?),
            _ => {}
        }
    }

    Some(Recurrence {
        freq: freq?,
        interval,
        count,
        until,
    })
}

impl Recurrence {
    /// Expands this rule into a sequence of occurrences starting at
    /// `dtstart` (included). Stops at the first of: `COUNT`, `UNTIL`, or
    /// `limit` occurrences - `limit` matters because a rule with neither
    /// `COUNT` nor `UNTIL` repeats forever.
    ///
    /// For `MONTHLY`/`YEARLY` rules, an interim date that doesn't exist
    /// (e.g. day 31 landing on April) is skipped rather than clamped,
    /// per RFC 5545.
    pub fn expand(&self, dtstart: &DateTimeValue, limit: usize) -> Vec<DateTimeValue> {
        let mut out = Vec::new();
        let mut current = dtstart.clone();
        let mut produced: u32 = 0;

        loop {
            if out.len() >= limit {
                break;
            }
            if let Some(count) = self.count {
                if produced >= count {
                    break;
                }
            }
            if let Some(until) = &self.until {
                if current.instant() > until.instant() {
                    break;
                }
            }

            out.push(current.clone());
            produced += 1;

            match advance_date(current.date(), self.freq, self.interval) {
                Some(next_date) => current = current.with_date(next_date),
                None => break,
            }
        }

        out
    }
}

impl DateTimeValue {
    fn date(&self) -> Date {
        match self {
            DateTimeValue::Date(d) => *d,
            DateTimeValue::DateTime { date, .. } => *date,
        }
    }

    fn instant(&self) -> (Date, Time) {
        match self {
            DateTimeValue::Date(d) => (*d, Time { hour: 0, minute: 0, second: 0 }),
            DateTimeValue::DateTime { date, time, .. } => (*date, *time),
        }
    }

    fn with_date(&self, date: Date) -> DateTimeValue {
        match self {
            DateTimeValue::Date(_) => DateTimeValue::Date(date),
            DateTimeValue::DateTime { time, kind, .. } => DateTimeValue::DateTime {
                date,
                time: *time,
                kind: kind.clone(),
            },
        }
    }
}

fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => unreachable!("month out of range: {month}"),
    }
}

/// Days since 1970-01-01, via Howard Hinnant's `days_from_civil`
/// algorithm (proleptic Gregorian, correct across the full `i32` year range).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { i64::from(year) - 1 } else { i64::from(year) };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Inverse of `days_from_civil`.
fn civil_from_days(z: i64) -> Date {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if month <= 2 { y + 1 } else { y };
    Date { year: year as i32, month, day }
}

fn add_days(date: Date, days: i64) -> Date {
    civil_from_days(days_from_civil(date.year, date.month, date.day) + days)
}

/// Adds a number of months, keeping the day of month. Returns `None`
/// when the target month doesn't have that day (e.g. day 31 in April).
fn add_months(date: Date, months: i64) -> Option<Date> {
    let total = i64::from(date.year) * 12 + i64::from(date.month - 1) + months;
    let year = total.div_euclid(12) as i32;
    let month = (total.rem_euclid(12) + 1) as u32;
    if date.day > days_in_month(year, month) {
        None
    } else {
        Some(Date { year, month, day: date.day })
    }
}

/// Advances `date` by one step of `freq`/`interval`.
fn advance_date(date: Date, freq: Freq, interval: u32) -> Option<Date> {
    let interval = i64::from(interval);
    match freq {
        Freq::Daily => Some(add_days(date, interval)),
        Freq::Weekly => Some(add_days(date, 7 * interval)),
        Freq::Monthly => advance_by_months(date, interval),
        Freq::Yearly => advance_by_months(date, interval * 12),
    }
}

/// Keeps adding another `step` months past a nonexistent target date
/// (RFC 5545's "that occurrence is skipped" rule) instead of clamping it.
/// Bounded to 48 tries, which comfortably covers any interval/day-of-month
/// combination since day-of-month can only ever land on so many months.
fn advance_by_months(date: Date, step: i64) -> Option<Date> {
    let mut total = step;
    for _ in 0..48 {
        if let Some(next) = add_months(date, total) {
            return Some(next);
        }
        total += step;
    }
    None
}

#[derive(Debug, Clone, Default)]
pub struct Event {
    pub uid: Option<String>,
    pub summary: Option<String>,
    pub dtstart: Option<DateTimeValue>,
    pub dtend: Option<DateTimeValue>,
    pub rrule: Option<Recurrence>,
}

#[derive(Debug, Clone, Default)]
pub struct Calendar {
    pub events: Vec<Event>,
}

/// Splits text into lines on `\r\n`, `\n`, or bare `\r`, without keeping
/// the terminator. iCalendar files are supposed to use CRLF but plenty
/// of real-world files don't bother.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                lines.push(&text[start..i]);
                i += 1;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
                start = i;
            }
            b'\n' => {
                lines.push(&text[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Reverses line folding: a line that starts with a single space or tab
/// is a continuation of the previous line, with that one leading
/// whitespace character removed (RFC 5545 section 3.1).
pub fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in split_lines(text) {
        let is_continuation = raw.starts_with(' ') || raw.starts_with('\t');
        if is_continuation && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&raw[1..]);
        } else {
            lines.push(raw.to_string());
        }
    }
    lines
}

/// Undoes the backslash escaping used inside property values: `\,`, `\;`,
/// `\\`, and `\n`/`\N` for newlines. An unrecognized escape is left as-is
/// rather than silently dropping the backslash, since a wrong guess here
/// is worse than a slightly odd passthrough.
pub fn unescape_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(',') => out.push(','),
            Some(';') => out.push(';'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Parses one unfolded content line into name, parameters, and value.
///
/// The tricky part is finding the colon that separates the parameter
/// list from the value: parameter values can be quoted, and a quoted
/// value is allowed to contain colons and semicolons of its own (this
/// shows up in things like `ALTREP` and `TZID` with a URI value).
pub fn parse_property(line: &str) -> Option<Property> {
    let mut in_quotes = false;
    let mut split_at = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                split_at = Some(i);
                break;
            }
            _ => {}
        }
    }
    let split_at = split_at?;
    let head = &line[..split_at];
    let raw_value = &line[split_at + 1..];

    let mut segments = head.split(';');
    let name = segments.next()?.to_string();
    if name.is_empty() {
        return None;
    }

    let mut params = Vec::new();
    for segment in segments {
        if let Some(eq) = segment.find('=') {
            let key = segment[..eq].to_string();
            let mut val = &segment[eq + 1..];
            if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
                val = &val[1..val.len() - 1];
            }
            params.push((key, val.to_string()));
        }
    }

    Some(Property {
        name,
        params,
        value: unescape_value(raw_value),
    })
}

/// Parses a whole .ics document into a list of VEVENT blocks. Anything
/// outside a BEGIN:VEVENT/END:VEVENT pair (VCALENDAR headers, VTIMEZONE,
/// VALARM, ...) is currently ignored. An event that never sees a matching
/// END:VEVENT is dropped rather than emitted half-built.
pub fn parse_calendar(text: &str) -> Calendar {
    let mut events = Vec::new();
    let mut current: Option<Event> = None;

    for line in unfold(text) {
        let Some(prop) = parse_property(&line) else {
            continue;
        };
        match prop.name.as_str() {
            "BEGIN" if prop.value == "VEVENT" => {
                current = Some(Event::default());
            }
            "END" if prop.value == "VEVENT" => {
                if let Some(event) = current.take() {
                    events.push(event);
                }
            }
            "UID" => {
                if let Some(event) = current.as_mut() {
                    event.uid = Some(prop.value);
                }
            }
            "SUMMARY" => {
                if let Some(event) = current.as_mut() {
                    event.summary = Some(prop.value);
                }
            }
            "DTSTART" => {
                if let Some(event) = current.as_mut() {
                    event.dtstart = parse_date_time(&prop);
                }
            }
            "DTEND" => {
                if let Some(event) = current.as_mut() {
                    event.dtend = parse_date_time(&prop);
                }
            }
            "RRULE" => {
                if let Some(event) = current.as_mut() {
                    event.rrule = parse_rrule(&prop.value);
                }
            }
            _ => {}
        }
    }

    Calendar { events }
}
