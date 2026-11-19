//! Same-sense matching from a bounded local dictionary. Match the user's words first.
use super::han;
use std::{
    collections::{BTreeSet, HashMap},
    sync::OnceLock,
};

const SOURCE: &str = include_str!("generated/chinese-synsets.tsv");
const MAX_GROUPS: usize = 4;
const MAX_ALIASES: usize = 4;

pub(super) struct Group {
    term: String,
    aliases: Vec<&'static str>,
}

fn dictionary() -> &'static HashMap<&'static str, Vec<&'static str>> {
    static TERMS: OnceLock<HashMap<&str, Vec<&str>>> = OnceLock::new();
    TERMS.get_or_init(|| {
        let mut terms: HashMap<&str, Vec<&str>> = HashMap::new();
        for line in SOURCE.lines().filter(|line| !line.starts_with('#')) {
            let group = line.split('\t').collect::<Vec<_>>();
            for word in &group {
                terms
                    .entry(word)
                    .or_default()
                    .extend(group.iter().copied().filter(|alias| alias != word));
            }
        }
        for words in terms.values_mut() {
            words.sort_unstable();
            words.dedup();
            words.truncate(MAX_ALIASES);
        }
        terms
    })
}

pub(super) fn groups(text: &str) -> Vec<Group> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for run in text.split(|c| !han(c)).filter(|part| part.chars().count() >= 2) {
        let dictionary = dictionary();
        let starts = run
            .char_indices()
            .map(|(at, _)| at)
            .chain(std::iter::once(run.len()))
            .collect::<Vec<_>>();
        let mut index = 0;
        while index + 1 < starts.len() && result.len() < MAX_GROUPS {
            let next = (2..=8.min(starts.len() - index - 1)).rev().find_map(|width| {
                let term = &run[starts[index]..starts[index + width]];
                Some((width, term, dictionary.get(term)?))
            });
            if let Some((width, term, aliases)) = next {
                if seen.insert(term) {
                    result.push(Group { term: term.to_owned(), aliases: aliases.clone() });
                }
                index += width;
            } else {
                index += 1;
            }
        }
        if result.len() == MAX_GROUPS {
            break;
        }
    }
    result
}

pub(super) fn aliases(groups: &[Group]) -> impl Iterator<Item = &str> {
    groups.iter().flat_map(|group| group.aliases.iter().map(|&word| word as &str))
}

/// A short query may stand on one strong dictionary hit. For a longer query, require a second
/// matching concept, or a synonym in the title. Never count the same alias as a second hit.
pub(super) fn find(groups: &[Group], exact: &str, title: &str, body: &str) -> Option<(u32, usize)> {
    let mut matched = BTreeSet::new();
    let mut best = (0, 0);
    for group in groups {
        if title.contains(&group.term) || body.contains(&group.term) {
            matched.insert(group.term.as_str());
        }
        for &alias in &group.aliases {
            let in_title = title.contains(alias);
            let at = body.find(alias);
            if (in_title || at.is_some()) && matched.insert(alias) {
                let strength = if in_title {
                    4
                } else if group.term == exact {
                    3
                } else {
                    1
                };
                if strength > best.0 {
                    best = (strength, at.unwrap_or(0));
                }
                break;
            }
        }
    }
    (best.0 >= 3 || (best.0 > 0 && matched.len() >= 2))
        .then_some((best.0 + matched.len() as u32, best.1))
}
