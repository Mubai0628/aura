use crate::{Entry, Limits, format, limits};

/// Parse a buffer while retaining source order, including the final entry at EOF.
pub fn parse(input: &str, limits: Limits) -> Vec<Entry> {
    if limits.max_records == 0 {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut pending: Option<Entry> = None;
    for line in input.lines() {
        if let Some(header) = format::header(line) {
            emit(&mut result, pending.take(), limits.max_records);
            if result.len() == limits.max_records {
                break;
            }
            let mut entry = Entry::empty(header.timestamp, header.level);
            entry.truncated = limits::append(&mut entry.text, header.message, limits.max_bytes);
            pending = Some(entry);
        } else if format::continuation(line).is_some() {
            // Legacy importer only understood one line per entry.
            continue;
        }
    }
    emit(&mut result, pending, limits.max_records);
    result
}

fn emit(result: &mut Vec<Entry>, pending: Option<Entry>, maximum: usize) {
    if result.len() < maximum {
        if let Some(entry) = pending {
            result.push(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_has_no_records() {
        assert!(parse("", Limits::default()).is_empty());
    }

    #[test]
    fn headers_keep_their_source_order() {
        let entries = parse("4 INFO second\n2 WARN first", Limits::default());
        assert_eq!(entries.iter().map(|entry| entry.timestamp).collect::<Vec<_>>(), [4, 2]);
    }

    #[test]
    fn malformed_and_empty_lines_do_not_create_entries() {
        let entries = parse("junk\n\n1 INFO ready\ninvalid\n", Limits::default());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "ready");
    }

    #[test]
    fn record_limit_keeps_the_first_entries() {
        let entries = parse("1 INFO a\n2 INFO b\n3 INFO c", Limits::preview(100, 2));
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].text, "b");
    }
}
