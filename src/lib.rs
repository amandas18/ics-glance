//! Minimal iCalendar (RFC 5545) reading support.
//!
//! This is not a full implementation of the spec. It handles the parts
//! that show up in real .ics files often enough to matter: folded lines,
//! CRLF/LF mixed line endings, quoted parameter values, and the standard
//! backslash escapes inside property values.

/// A single unfolded, parsed content line: `NAME;PARAM=VAL;...:VALUE`.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct Event {
    pub uid: Option<String>,
    pub summary: Option<String>,
    pub dtstart: Option<String>,
    pub dtend: Option<String>,
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
                    event.dtstart = Some(prop.value);
                }
            }
            "DTEND" => {
                if let Some(event) = current.as_mut() {
                    event.dtend = Some(prop.value);
                }
            }
            _ => {}
        }
    }

    Calendar { events }
}
