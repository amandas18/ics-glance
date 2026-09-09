use ics_glance::{
    parse_calendar, parse_date_time, parse_property, unescape_value, unfold, DateTimeValue,
    TimeKind,
};

#[test]
fn unfold_handles_awkward_line_endings_and_folding() {
    struct Case {
        name: &'static str,
        input: &'static str,
        want: &'static [&'static str],
    }

    let cases: &[Case] = &[
        Case {
            name: "space continuation",
            input: "SUMMARY:Team\n meeting\n",
            want: &["SUMMARY:Teammeeting"],
        },
        Case {
            name: "tab continuation",
            input: "SUMMARY:Team\n\tmeeting",
            want: &["SUMMARY:Teammeeting"],
        },
        Case {
            name: "crlf line endings, no folding",
            input: "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n",
            want: &["BEGIN:VCALENDAR", "END:VCALENDAR"],
        },
        Case {
            name: "bare lf, no folding",
            input: "UID:abc\nSUMMARY:x",
            want: &["UID:abc", "SUMMARY:x"],
        },
        Case {
            name: "fold across three physical lines",
            input: "DESCRIPTION:one\n two\n three",
            want: &["DESCRIPTION:onetwothree"],
        },
    ];

    for case in cases {
        let got = unfold(case.input);
        assert_eq!(got, case.want, "case failed: {}", case.name);
    }
}

#[test]
fn unescape_value_handles_backslash_sequences() {
    struct Case {
        name: &'static str,
        input: &'static str,
        want: &'static str,
    }

    let cases: &[Case] = &[
        Case { name: "escaped comma", input: r"a\,b", want: "a,b" },
        Case { name: "escaped semicolon", input: r"a\;b", want: "a;b" },
        Case { name: "escaped backslash", input: r"a\\b", want: "a\\b" },
        Case { name: "escaped newline lowercase n", input: r"a\nb", want: "a\nb" },
        Case { name: "escaped newline uppercase N", input: r"a\Nb", want: "a\nb" },
        Case { name: "unrecognized escape kept literal", input: r"a\qb", want: r"a\qb" },
        Case { name: "trailing lone backslash", input: r"a\", want: r"a\" },
        Case { name: "no escapes at all", input: "plain text", want: "plain text" },
    ];

    for case in cases {
        let got = unescape_value(case.input);
        assert_eq!(got, case.want, "case failed: {}", case.name);
    }
}

#[test]
fn parse_property_splits_name_params_and_value() {
    struct Case {
        name: &'static str,
        line: &'static str,
        want_name: &'static str,
        want_value: &'static str,
        want_params: &'static [(&'static str, &'static str)],
    }

    let cases: &[Case] = &[
        Case {
            name: "plain value, no params",
            line: "UID:1234-5678",
            want_name: "UID",
            want_value: "1234-5678",
            want_params: &[],
        },
        Case {
            name: "single param",
            line: "DTSTART;VALUE=DATE:20260101",
            want_name: "DTSTART",
            want_value: "20260101",
            want_params: &[("VALUE", "DATE")],
        },
        Case {
            name: "quoted param value with a colon is not the value separator",
            line: r#"ATTACH;FMTTYPE=text/plain;X-URI="http://example.com:8080/x":https://example.com/real"#,
            want_name: "ATTACH",
            want_value: "https://example.com/real",
            want_params: &[("FMTTYPE", "text/plain"), ("X-URI", "http://example.com:8080/x")],
        },
        Case {
            name: "value with an escaped comma is unescaped",
            line: r"SUMMARY:Coffee\, then lunch",
            want_name: "SUMMARY",
            want_value: "Coffee, then lunch",
            want_params: &[],
        },
    ];

    for case in cases {
        let prop = parse_property(case.line)
            .unwrap_or_else(|| panic!("case failed to parse at all: {}", case.name));
        assert_eq!(prop.name, case.want_name, "case failed (name): {}", case.name);
        assert_eq!(prop.value, case.want_value, "case failed (value): {}", case.name);
        let want_params: Vec<(String, String)> = case
            .want_params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(prop.params, want_params, "case failed (params): {}", case.name);
    }
}

#[test]
fn parse_calendar_extracts_events_table_driven() {
    struct Case {
        name: &'static str,
        input: &'static str,
        want_summaries: &'static [&'static str],
    }

    let cases: &[Case] = &[
        Case {
            name: "single event",
            input: "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:1\r\nSUMMARY:Standup\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            want_summaries: &["Standup"],
        },
        Case {
            name: "two events back to back",
            input: "BEGIN:VEVENT\nSUMMARY:One\nEND:VEVENT\nBEGIN:VEVENT\nSUMMARY:Two\nEND:VEVENT\n",
            want_summaries: &["One", "Two"],
        },
        Case {
            name: "unterminated event is dropped, not half-emitted",
            input: "BEGIN:VEVENT\nSUMMARY:Orphan\n",
            want_summaries: &[],
        },
        Case {
            name: "empty calendar has no events",
            input: "BEGIN:VCALENDAR\nEND:VCALENDAR\n",
            want_summaries: &[],
        },
        Case {
            name: "folded summary inside an event",
            input: "BEGIN:VEVENT\nSUMMARY:Long\n title\nEND:VEVENT\n",
            want_summaries: &["Longtitle"],
        },
    ];

    for case in cases {
        let calendar = parse_calendar(case.input);
        let got: Vec<&str> = calendar
            .events
            .iter()
            .map(|e| e.summary.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(got, case.want_summaries, "case failed: {}", case.name);
    }
}

#[test]
fn parse_date_time_handles_date_and_datetime_shapes() {
    struct Case {
        name: &'static str,
        line: &'static str,
        want: Option<&'static str>,
    }

    let cases: &[Case] = &[
        Case {
            name: "explicit VALUE=DATE",
            line: "DTSTART;VALUE=DATE:20260101",
            want: Some("2026-01-01"),
        },
        Case {
            name: "bare date value with no VALUE param",
            line: "DTSTART:20260101",
            want: Some("2026-01-01"),
        },
        Case {
            name: "floating date-time, no Z and no TZID",
            line: "DTSTART:20260112T090000",
            want: Some("2026-01-12 09:00:00"),
        },
        Case {
            name: "utc date-time",
            line: "DTSTART:20260112T090000Z",
            want: Some("2026-01-12 09:00:00 UTC"),
        },
        Case {
            name: "date-time with TZID",
            line: "DTSTART;TZID=America/New_York:20260112T090000",
            want: Some("2026-01-12 09:00:00 America/New_York"),
        },
        Case {
            name: "malformed date is rejected",
            line: "DTSTART;VALUE=DATE:2026-01-01",
            want: None,
        },
        Case {
            name: "hour out of range is rejected",
            line: "DTSTART:20260112T250000",
            want: None,
        },
    ];

    for case in cases {
        let prop = parse_property(case.line)
            .unwrap_or_else(|| panic!("property failed to parse at all: {}", case.name));
        let got = parse_date_time(&prop).map(|v| v.to_string());
        assert_eq!(
            got.as_deref(),
            case.want,
            "case failed: {}",
            case.name
        );
    }
}

#[test]
fn parse_date_time_reports_timezone_kind() {
    let floating = parse_property("DTSTART:20260112T090000").unwrap();
    match parse_date_time(&floating) {
        Some(DateTimeValue::DateTime { kind, .. }) => assert_eq!(kind, TimeKind::Floating),
        other => panic!("expected floating date-time, got {other:?}"),
    }

    let utc = parse_property("DTSTART:20260112T090000Z").unwrap();
    match parse_date_time(&utc) {
        Some(DateTimeValue::DateTime { kind, .. }) => assert_eq!(kind, TimeKind::Utc),
        other => panic!("expected utc date-time, got {other:?}"),
    }

    let zoned = parse_property("DTSTART;TZID=America/Chicago:20260112T090000").unwrap();
    match parse_date_time(&zoned) {
        Some(DateTimeValue::DateTime { kind, .. }) => {
            assert_eq!(kind, TimeKind::Zone("America/Chicago".to_string()))
        }
        other => panic!("expected zoned date-time, got {other:?}"),
    }
}
