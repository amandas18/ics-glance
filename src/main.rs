use std::env;
use std::fs;
use std::process::ExitCode;

use ics_glance::parse_calendar;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: ics-glance <file.ics>");
            return ExitCode::FAILURE;
        }
    };

    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error reading {path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let calendar = parse_calendar(&text);
    if calendar.events.is_empty() {
        println!("no events found in {path}");
        return ExitCode::SUCCESS;
    }

    for event in &calendar.events {
        let start = event
            .dtstart
            .as_ref()
            .map(|d| d.to_string())
            .unwrap_or_else(|| "(no start)".to_string());
        let summary = event.summary.as_deref().unwrap_or("(no summary)");
        println!("{start}  {summary}");
    }

    ExitCode::SUCCESS
}
