//! Extract explicit original-value requirements from authenticated user wording.
use aura_domain_common::WorkspacePath;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactArtifactValue {
    pub path: WorkspacePath,
    pub text: String,
    title: bool,
    title_only: bool,
}

impl ExactArtifactValue {
    pub fn matches(&self, document: &str) -> bool {
        if self.title_only {
            return document == self.text
                || document.strip_suffix("\r\n").or_else(|| document.strip_suffix('\n'))
                    == Some(self.text.as_str());
        }
        if !self.title {
            return document.contains(&self.text);
        }
        let titles = || document.lines().map(|line| line.trim().trim_start_matches('#').trim());
        let same = |candidate: &str| {
            candidate == self.text
                || (quote(candidate) == Some(self.text.as_str())
                    && candidate.chars().count() == self.text.chars().count() + 2)
        };
        let mut named = titles().filter_map(labeled_title).peekable();
        if named.peek().is_some() { named.all(same) } else { titles().any(same) }
    }

    pub fn set_title_only(values: &mut Vec<Self>, path: WorkspacePath, text: String) {
        if let Some(value) = values.iter_mut().find(|value| value.path == path && value.title) {
            value.text = text;
            value.title_only = true;
        } else if values.len() < 4 {
            values.push(Self { path, text, title: true, title_only: true });
        }
    }
}

fn labeled_title(line: &str) -> Option<&str> {
    ["题名", "标题", "题目", "论文题目", "选题名称", "Title", "title"].into_iter().find_map(
        |label| {
            let tail = line.strip_prefix(label)?.trim_start();
            Some(tail.strip_prefix(['：', ':'])?.trim())
        },
    )
}

const ORIGINAL: &[&str] = &[
    "逐字保留",
    "原样保留",
    "原文保留",
    "按原文保留",
    "原样用",
    "原样使用",
    "keep verbatim",
    "preserve verbatim",
];
const UPDATE: &[&str] =
    &["题名改为", "标题改为", "题目改为", "题名更正为", "标题更正为", "change the title to"];
const APPROVED_TITLE: &[&str] =
    &["题名定为", "标题定为", "题名为", "标题为", "the approved title is"];
const LATEST_TITLE_ONLY: &[&str] = &[
    "只写我最新指定的完整题名",
    "只写我最新指定的题名",
    "只写我最新确认的完整题名",
    "只写我最新确认的题名",
    "write only my latest approved title",
    "write only the latest title i specified",
];
const TITLE: &[&str] = &["题名", "标题", "题目", "title"];
const NEGATED: &[&str] =
    &["不需要", "不用", "不要", "不必", "无需", "无须", "别", "do not", "don't"];

const DESTINATIONS: &[&str] =
    &["写入", "保存到", "保存为", "存为", "输出到", "write to", "save to", "save as"];

/// A current request to put only the user's latest approved title in one artifact.
pub fn latest_title_only_target(message: &str) -> Option<WorkspacePath> {
    let mut targets = direct_clauses(message).into_iter().filter_map(|clause| {
        marker(&clause.to_ascii_lowercase(), LATEST_TITLE_ONLY)?;
        let mut files = paths(clause).into_iter();
        files.next().filter(|_| files.next().is_none())
    });
    targets.next().filter(|_| targets.next().is_none())
}

/// The newest explicitly assigned title in an original user message.
pub fn explicit_title(message: &str) -> Option<&str> {
    let trimmed = message.trim_start();
    if trimmed.starts_with("/compact") || trimmed.starts_with("/init") {
        return None;
    }
    direct_clauses(message).into_iter().rev().find_map(|clause| {
        let lower = clause.to_ascii_lowercase();
        let end = marker(&lower, UPDATE).or_else(|| marker(&lower, APPROVED_TITLE))?;
        let (title, rest) = quoted_value(&clause[end..])?;
        (!rest.trim().starts_with(['吗', '么'])).then_some(title)
    })
}

pub fn apply_exact_artifact_values(values: &mut Vec<ExactArtifactValue>, message: &str) {
    let clauses = direct_clauses(message);
    let mut destinations = clauses.iter().enumerate().filter_map(|(index, clause)| {
        let lower = clause.to_ascii_lowercase();
        let end = marker(&lower, DESTINATIONS)?;
        let paths = paths(&clause[end..]);
        (!paths.is_empty()).then_some((index, paths))
    });
    let adjacent =
        destinations.next().filter(|(_, paths)| paths.len() == 1 && destinations.next().is_none());
    for (index, clause) in clauses.iter().enumerate() {
        let lower = clause.to_ascii_lowercase();
        let correction = marker(&lower, UPDATE);
        let original = marker(&lower, ORIGINAL);
        let Some(end) = correction.or(original) else { continue };
        let tail = clause[end..].trim_start();
        let title_value = TITLE.iter().find_map(|label| tail.strip_prefix(label));
        let Some((text, after_value)) = quoted_value(title_value.unwrap_or(tail)) else { continue };
        let title = correction.is_some()
            || title_value.is_some()
            || TITLE.iter().any(|word| lower[..end].contains(word));
        let mut targets = paths(&clause[..end]);
        for path in paths(after_value) {
            if !targets.contains(&path) {
                targets.push(path);
            }
        }
        let mut files = targets.into_iter();
        let first = files.next();
        if files.next().is_some() {
            continue;
        }
        let path = if let Some(path) = first {
            path
        } else if let Some((_, paths)) = adjacent.as_ref().filter(|(at, _)| at.abs_diff(index) == 1)
        {
            paths[0].clone()
        } else if correction.is_some() {
            let mut titles = values.iter().filter(|value| value.title);
            let Some(old) = titles.next() else { continue };
            if titles.next().is_some() {
                continue;
            }
            old.path.clone()
        } else {
            continue;
        };
        if let Some(prior) = values
            .iter_mut()
            .find(|prior| prior.path == path && ((prior.title && title) || prior.text == text))
        {
            prior.text = text.to_owned();
        } else if values.len() < 4 {
            values.push(ExactArtifactValue {
                path,
                text: text.to_owned(),
                title,
                title_only: false,
            });
        }
    }
}

fn direct_clauses(message: &str) -> Vec<&str> {
    let mut clauses = Vec::new();
    let mut fence = None;
    for line in message.lines().take(120).map(str::trim) {
        let delimiter = ["```", "~~~"].into_iter().find(|marker| line.starts_with(marker));
        if let Some(marker) = delimiter {
            if fence.is_none() {
                fence = Some(marker);
            } else if fence == Some(marker) {
                fence = None;
            }
            // Do not bind instructions across a quoted example.
            clauses.push("");
            continue;
        }
        if fence.is_some() || line.starts_with('>') || line.len() > 4096 {
            clauses.push("");
            continue;
        }
        let mut start = 0;
        let mut quoted = None;
        for (at, ch) in line.char_indices() {
            if let Some(end) = quoted {
                if ch == end {
                    quoted = None;
                }
            } else if let Some(end) = closing_quote(ch).or_else(|| (ch == '`').then_some('`')) {
                quoted = Some(end);
            } else if "。；！？;".contains(ch)
                || (ch == '.' && line[at + 1..].starts_with(char::is_whitespace))
            {
                if !line[start..at].trim().is_empty() {
                    clauses.push(line[start..at].trim());
                }
                start = at + ch.len_utf8();
            }
        }
        if !line[start..].trim().is_empty() {
            clauses.push(line[start..].trim());
        }
    }
    clauses
}

fn marker(clause: &str, markers: &[&str]) -> Option<usize> {
    let mut quoted = None;
    let mut found = None;
    let mut next_marker = 0;
    for (at, ch) in clause.char_indices() {
        if at < next_marker {
            continue;
        }
        if let Some(end) = quoted {
            if ch == end {
                quoted = None;
            }
            continue;
        }
        quoted = closing_quote(ch).or_else(|| (ch == '`').then_some('`'));
        let Some(word) = markers
            .iter()
            .filter(|word| clause[at..].starts_with(**word))
            .max_by_key(|word| word.len())
        else {
            continue;
        };
        next_marker = at + word.len();
        let lead = clause[..at].rsplit([',', '，']).next().unwrap_or_default().trim_end();
        let negated = lead.trim_end_matches('再').trim_end();
        if NEGATED.iter().any(|prefix| negated.ends_with(prefix))
            || [
                "手册建议",
                "模板建议",
                "手册示例",
                "模板示例",
                "举例",
                "例如",
                "for example",
                "the template recommends",
                "the manual suggests",
            ]
            .iter()
            .any(|example| lead.contains(example))
        {
            continue;
        }
        found = Some(at + word.len());
    }
    found
}

fn closing_quote(open: char) -> Option<char> {
    Some(match open {
        '《' => '》',
        '“' => '”',
        '「' => '」',
        '『' => '』',
        '"' => '"',
        _ => return None,
    })
}

fn quote(tail: &str) -> Option<&str> {
    quoted_value(tail).map(|(value, _)| value)
}

fn quoted_value(tail: &str) -> Option<(&str, &str)> {
    let tail = tail.trim_start_matches(|c: char| c.is_whitespace() || "：:".contains(c));
    let open = tail.chars().next()?;
    let close = closing_quote(open)?;
    let (value, rest) = tail[open.len_utf8()..].split_once(close)?;
    (value == value.trim()
        && (1..=200).contains(&value.chars().count())
        && !value.chars().any(char::is_control))
    .then_some((value, rest))
}

fn paths(clause: &str) -> Vec<WorkspacePath> {
    let mut found = Vec::new();
    for (index, section) in clause.split('`').enumerate() {
        if index % 2 == 1 {
            add_path(section, &mut found);
        } else {
            for word in section
                .split(|c: char| c.is_whitespace() || "，,:：()（）<>《》“”「」\"".contains(c))
            {
                add_path(word.trim_end_matches('.'), &mut found);
            }
        }
    }
    found
}

fn add_path(word: &str, found: &mut Vec<WorkspacePath>) {
    let word = DESTINATIONS
        .iter()
        .find_map(|prefix| word.trim_start_matches('请').strip_prefix(*prefix))
        .unwrap_or(word);
    let Some((_, extension)) = word.rsplit_once('.') else { return };
    if !matches!(extension.to_ascii_lowercase().as_str(), "txt" | "md" | "docx") {
        return;
    }
    if let Ok(path) = WorkspacePath::parse(word)
        && !path.as_str().split('/').any(|part| part.starts_with('.'))
        && !found.contains(&path)
    {
        found.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_and_its_only_delivery_path_may_be_in_adjacent_sentences() {
        let mut values = Vec::new();
        apply_exact_artifact_values(
            &mut values,
            "请把封面写入 `封面.md`。\n题名请原样用《正式题目：甲；乙》。",
        );
        assert_eq!(values.first().map(|v| v.path.as_str()), Some("封面.md"));
        assert!(values[0].matches("题目：正式题目：甲；乙"));
        assert!(!values[0].matches("题目：短题名\n\n资料中的示例\n正式题目：甲；乙"));
        apply_exact_artifact_values(&mut values, "题名改为《新题名》。");
        assert!(values[0].matches("# 新题名"));
    }

    #[test]
    fn separate_examples_or_multiple_delivery_paths_do_not_bind_a_title() {
        let mut values = Vec::new();
        for input in [
            "参考资料在 manual.md。题名须逐字保留《原文》。",
            "保存到 first.md 或 second.md。题名须逐字保留《原文》。",
            "请写入 first.md。\n```\n标题改为《示例》\n```",
        ] {
            apply_exact_artifact_values(&mut values, input);
        }
        assert!(values.is_empty());
    }

    #[test]
    fn uses_the_latest_explicit_title_and_ignores_general_guidance() {
        let mut values = Vec::new();
        apply_exact_artifact_values(
            &mut values,
            "封面题名须逐字保留《资料检索与证据核验研究》，请写入 cover.txt，交付前核对。",
        );
        assert_eq!(values[0].text, "资料检索与证据核验研究");
        apply_exact_artifact_values(
            &mut values,
            "参考手册建议把题名控制在十字以内。\n> 题名改为《示例》",
        );
        apply_exact_artifact_values(
            &mut values,
            "模板里写的是“标题改为《引用的示例》”。手册建议题名改为《简称》。",
        );
        assert_eq!(values[0].text, "资料检索与证据核验研究");
        apply_exact_artifact_values(&mut values, "标题改为《资料核验与检索》");
        assert_eq!(values[0].text, "资料核验与检索");
        assert_eq!(values[0].path.as_str(), "cover.txt");
        assert!(values[0].matches("题名：《资料核验与检索》"));
        assert!(!values[0].matches("题名：《资料核验与检索》正式稿"));
    }

    #[test]
    fn general_questions_and_ambiguous_or_external_targets_do_not_open_files() {
        let mut values = Vec::new();
        for input in [
            "《某个选题》如何缩短？",
            "标题逐字保留《内容》参考 a.md b.md",
            "原样保留《秘密》 ../a.txt",
            "原样保留《秘密》 C:/other.txt",
            "题名不需要逐字保留《示例》，写入 cover.txt。",
            "不必按原文保留《示例》，写入 cover.txt。",
        ] {
            apply_exact_artifact_values(&mut values, input);
        }
        assert!(values.is_empty());
    }

    #[test]
    fn a_literal_title_only_reference_resolves_one_destination_without_formatting() {
        let target =
            latest_title_only_target("在 cover.txt 中只写我最新指定的完整题名，逐字保留。")
                .expect("explicit title destination");
        let mut values = vec![];
        ExactArtifactValue::set_title_only(&mut values, target, "最新题名、终稿".into());
        assert!(values[0].matches("最新题名、终稿\r\n"));
        assert!(!values[0].matches("《最新题名、终稿》\n"));
        assert!(!values[0].matches(" 最新题名、终稿\n"));
        assert!(!values[0].matches("题名：最新题名、终稿"));
        apply_exact_artifact_values(&mut values, "题名改为《再次修订》。");
        assert!(values[0].matches("再次修订\n"));
        assert_eq!(
            explicit_title("初始题名为《第一题》。题名改为《第二题、终稿》。"),
            Some("第二题、终稿")
        );
        assert_eq!(explicit_title("手册示例题名改为《示例》。\n> 题名改为《引用》"), None);
        assert_eq!(latest_title_only_target("在 a.txt 或 b.txt 只写我最新指定的完整题名。"), None);
    }
}
