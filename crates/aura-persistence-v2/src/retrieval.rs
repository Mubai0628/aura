//! Bounded local matching for Vault notes and conversation sources.
use std::collections::BTreeSet;
mod synonyms;

const MAX_TERMS: usize = 32;
const STOP: &[&str] = &[
    "the",
    "please",
    "this",
    "that",
    "these",
    "with",
    "your",
    "what",
    "上一",
    "上次",
    "之前",
    "这些",
    "那个",
    "一下",
    "帮我",
    "请你",
    "继续",
    "之前的",
    "好的",
    "你好",
    "您好",
    "收到",
    "谢谢",
    "嗯嗯",
];

pub struct QueryTerms {
    exact: String,
    words: Vec<String>,
    synonyms: Vec<synonyms::Group>,
}

#[derive(Clone)]
pub struct Passage {
    pub score: u32,
    pub excerpt: String,
}

fn han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{9fff}' | '\u{20000}'..='\u{323af}')
}

fn lower(text: &str) -> String {
    text.chars().map(|ch| ch.to_lowercase().next().unwrap_or(ch)).collect()
}

impl QueryTerms {
    pub fn new(input: &str) -> Self {
        let exact = lower(&input.trim().chars().take(320).collect::<String>());
        let mut words = Vec::new();
        let mut seen = BTreeSet::new();
        let mut add = |text: &str| {
            if words.len() < MAX_TERMS
                && text.chars().count() >= 2
                && !STOP.contains(&text)
                && seen.insert(text.to_owned())
            {
                words.push(text.to_owned());
            }
        };
        let runs = exact.split(|c: char| !c.is_alphanumeric() && !matches!(c, '_' | '-' | '.'));
        for run in runs {
            if run.chars().count() <= 48 {
                add(run);
            }
        }
        for run in exact.split(|c| !han(c)).filter(|run| !run.is_empty()) {
            let chars = run.chars().collect::<Vec<_>>();
            // A two-character Chinese word needs a literal fallback; trigrams use the index.
            for width in [3, 2] {
                for term in chars.windows(width) {
                    add(&term.iter().collect::<String>());
                }
            }
        }
        Self { exact, words, synonyms: Vec::new() }
    }

    #[must_use]
    pub fn with_synonyms(mut self) -> Self {
        if !self.is_empty() {
            self.synonyms = synonyms::groups(&self.exact);
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn fts(&self) -> String {
        self.words
            .iter()
            .map(String::as_str)
            .chain(synonyms::aliases(&self.synonyms))
            .map(|word| {
                let escaped = word.replace('"', "\"\"");
                if word.chars().count() == 2 {
                    format!("short_terms:\"_{escaped}\"")
                } else {
                    format!("{{relative_path title body}}:\"{escaped}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" OR ")
    }

    pub fn passage(&self, name: &str, body: &str, limit: usize) -> Option<Passage> {
        if self.is_empty() {
            return None;
        }
        let title = lower(name);
        let match_body = lower(body);
        let mut score: u32 = 0;
        let mut found = 0;
        let mut best = (0, 0);
        for word in &self.words {
            let title_hit = title.contains(word);
            let body_hit = match_body.find(word);
            if !title_hit && body_hit.is_none() {
                continue;
            }
            found += 1;
            let weight = word.chars().count().min(12) as u32;
            score += weight * if title_hit { 3 } else { 1 };
            if let Some(byte) = body_hit
                && weight > best.0
            {
                best = (weight, byte);
            }
        }
        if found == 0 || (found == 1 && score < 3 && self.words.len() > 1) {
            let (weight, byte) = synonyms::find(&self.synonyms, &self.exact, &title, &match_body)?;
            score = weight;
            best.1 = byte;
        } else if title.contains(&self.exact) || match_body.contains(&self.exact) {
            score += 24;
        }
        let matched = match_body[..best.1].chars().count();
        let start = matched.saturating_sub(limit / 4);
        let excerpt = body.chars().skip(start).take(limit).collect();
        Some(Passage { score, excerpt })
    }
}

/// Expand a search using an explicit same-entity relationship in an authorized source.
/// A source may call the same thing by a formal name and a shorter one, in either order.
pub fn related_names(source: &str, query: &QueryTerms) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines().take(12) {
        for marker in ["又称", "也称为", "简称", "also known as", "abbreviated as"] {
            let Some((left, right)) = line.split_once(marker) else { continue };
            let trim = |s: &str| {
                s.trim_matches(|c: char| c.is_whitespace() || "：:()（）「」『』\"#“”".contains(c))
                    .to_owned()
            };
            let left = trim(left.rsplit(['，', '。', '、', ';', '；']).next().unwrap_or(left));
            let right = trim(right.split(['，', '。', '、', ';', '；']).next().unwrap_or(right));
            for (seed, alias) in [(&left, &right), (&right, &left)] {
                if query.passage(seed, "", 0).is_some()
                    && (2..=36).contains(&alias.chars().count())
                    && query.passage(alias, "", 0).is_none()
                    && !names.contains(alias)
                {
                    names.push(alias.clone());
                }
            }
        }
        if names.len() >= 2 {
            break;
        }
    }
    names.truncate(2);
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_synsets_match_a_semantic_rename_without_an_explicit_alias() {
        let title = "存档目录";
        let body = "项目要求详见手册。";
        assert!(QueryTerms::new("指南").passage(title, body, 80).is_none());
        let query = QueryTerms::new("指南").with_synonyms();
        let synonym = query.passage(title, body, 80).expect("synonym passage");
        assert!(synonym.excerpt.contains("手册"));
        assert!(query.passage("晚餐", "红烧排骨", 80).is_none());
        let exact = query.passage("直接命中指南", body, 80).expect("exact passage");
        assert!(exact.score > synonym.score);
    }

    #[test]
    fn unquoted_words_may_be_separated_and_reordered() {
        let query = QueryTerms::new("工作区 参考文献");
        let body = format!("{}参考文献在本工作区索引。", "略".repeat(500));
        let hit = query.passage("项目笔记", &body, 80).expect("passage matching both terms");
        assert!(hit.excerpt.contains("参考文献"));
        assert!(query.passage("与此无关", "你好", 80).is_none());
    }

    #[test]
    fn matching_sources_can_supply_an_alternate_name() {
        assert_eq!(
            related_names("排版规约又称交付外观。", &QueryTerms::new("排版规约")),
            vec!["交付外观"]
        );
        assert!(related_names("排版规约比较复杂。", &QueryTerms::new("排版规约")).is_empty());
    }
}
