//! A small import library for timestamped diagnostic logs.
//!
//! Each exported entry has a timestamp, a severity, and printable message text.
//! Callers choose limits before parsing so a preview can retain only a bounded
//! number of entries. The library does not perform I/O or depend on a locale.

mod entry;
mod format;
mod limits;
mod parser;

pub use entry::{Entry, Level};
pub use limits::Limits;
pub use parser::parse;

/// Select entries at or above a severity without changing their order.
pub fn at_least(entries: &[Entry], minimum: Level) -> Vec<&Entry> {
    entries.iter().filter(|entry| entry.level >= minimum).collect()
}

/// Produce a preview that can be displayed without retaining the input buffer.
pub fn preview(text: &str, limits: Limits, minimum: Level) -> Vec<String> {
    let entries = parse(text, limits);
    at_least(&entries, minimum).into_iter().map(Entry::display).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_filters_severity() {
        let result = preview("1 INFO ready\n2 ERROR failed", Limits::default(), Level::Warn);
        assert_eq!(result, vec!["2 ERROR failed"]);
    }
}
