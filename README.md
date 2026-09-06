# ics-glance

A small Rust library for reading iCalendar (`.ics`) files, plus a thin CLI
on top of it. No dependencies, standard library only.

## Why

Most calendar exports look fine until they don't. A `SUMMARY` gets folded
across two physical lines because it ran past 75 octets. A `TZID` parameter
is quoted and happens to contain a colon. Some exporters use bare `\n`
line endings instead of the `\r\n` the spec asks for. None of this is
exotic - it's just what real `.ics` files from real calendar software
actually look like - but it's easy to write a parser that only handles the
tidy case and breaks the first time it sees a real file.

This library handles line unfolding, quoted parameter values, and the
standard backslash escapes (`\,`, `\;`, `\\`, `\n`) as first-class cases,
not afterthoughts. It currently reads `VEVENT` blocks (`UID`, `SUMMARY`,
`DTSTART`, `DTEND`) out of a calendar; it does not write `.ics` files or
resolve recurrence rules yet.

## Library usage

```rust
use ics_glance::parse_calendar;

let text = std::fs::read_to_string("calendar.ics").unwrap();
let calendar = parse_calendar(&text);

for event in &calendar.events {
    println!(
        "{} - {}",
        event.dtstart.as_deref().unwrap_or("?"),
        event.summary.as_deref().unwrap_or("(untitled)"),
    );
}
```

Lower-level pieces are exported too, if you just need one part of this:

- `unfold(text)` - reverses RFC 5545 line folding, tolerating CRLF, LF,
  and bare CR line endings.
- `parse_property(line)` - splits one unfolded line into name,
  parameters, and value, respecting quoted parameter values.
- `unescape_value(value)` - undoes the backslash escaping used inside
  property values.

## CLI usage

```console
$ cat meeting.ics
BEGIN:VCALENDAR
BEGIN:VEVENT
UID:1
SUMMARY:Weekly sync
DTSTART:20260112T090000
END:VEVENT
END:VCALENDAR
$ cargo run -- meeting.ics
20260112T090000  Weekly sync
```

## Building

```console
$ cargo build
$ cargo test
```

## Status

Early. See the test suite in `tests/table_driven.rs` for the specific
edge cases this is meant to hold up against.
