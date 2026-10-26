//! The ranking of a path for a quick-open query: exact path, exact name, name prefix, a path
//! component prefix, then a subsequence with the fewest gaps; recently used paths rank higher.
use super::scan::CatalogEntry;

pub(super) fn fuzzy_score(entry: &CatalogEntry, query: &str, recent: Option<u64>) -> Option<i64> {
    if query.is_empty() {
        return Some(10_000 + recent.map_or(0, |value| i64::try_from(value.min(999)).unwrap_or(0)));
    }
    let path = entry.path.as_str().to_lowercase();
    let name = entry.file_name.to_lowercase();
    let mut score = if path == query {
        100_000
    } else if name == query {
        90_000
    } else if name.starts_with(query) {
        80_000 - i64::try_from(name.len().saturating_sub(query.len())).unwrap_or(10_000)
    } else if path.split('/').any(|component| component.starts_with(query)) {
        70_000
    } else {
        let gaps = subsequence_gaps(&path, query)?;
        60_000 - i64::try_from(gaps).unwrap_or(50_000)
    };
    score += recent.map_or(0, |value| i64::try_from(value.min(999)).unwrap_or(0));
    Some(score)
}

fn subsequence_gaps(value: &str, query: &str) -> Option<usize> {
    let mut positions = Vec::new();
    let mut search_start = 0;
    for query_character in query.chars() {
        let found = value[search_start..]
            .char_indices()
            .find(|(_, character)| *character == query_character)?;
        let position = search_start + found.0;
        positions.push(position);
        search_start = position + found.1.len_utf8();
    }
    positions
        .first()
        .zip(positions.last())
        .map(|(first, last)| last.saturating_sub(*first).saturating_sub(query.len()))
}
