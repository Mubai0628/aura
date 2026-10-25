//! `SKILL.md`: YAML front matter between `---` lines, then Markdown instructions. The workspace
//! has no YAML library, so this reads the subset Agent Skills use: top-level `key: value`
//! (plain, quoted, `|`/`>` blocks, `[a, b]` and `- a` lists) and nested blocks kept as written.
//! Anything else is refused with its line, never guessed.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_DESCRIPTION_CHARS: usize = 1024;
const MAX_COMPATIBILITY_CHARS: usize = 500;

/// What a Skill says about itself. None of it grants anything.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    /// The tools the Skill says it needs. Aura shows them; it never loads or allows a tool
    /// because a Skill names it.
    pub allowed_tools: Vec<String>,
    /// Other top-level keys (for example `metadata`), named but not interpreted.
    pub other_keys: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedSkill {
    pub frontmatter: SkillFrontmatter,
    /// The Markdown after the front matter.
    pub body: String,
    /// Things that do not stop the Skill from being used.
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Value {
    Text(String),
    List(Vec<String>),
    /// A nested block or flow mapping, kept as written.
    Raw,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// A plain scalar without its trailing ` # comment`.
fn plain(text: &str) -> String {
    let mut quoted = false;
    let bytes = text.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'#' && !quoted && (index == 0 || bytes[index - 1] == b' ') {
            return text[..index].trim().to_owned();
        }
        quoted ^= *byte == b'"';
    }
    text.trim().to_owned()
}

fn double_quoted(text: &str, line: usize) -> Result<(String, usize), String> {
    let mut out = String::new();
    let mut chars = text.char_indices().skip(1);
    while let Some((index, ch)) = chars.next() {
        match ch {
            '"' => return Ok((out, index + 1)),
            '\\' => match chars.next().map(|(_, c)| c) {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                Some('u') => {
                    let hex: String = (0..4).filter_map(|_| chars.next().map(|(_, c)| c)).collect();
                    let code = u32::from_str_radix(&hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or(format!("第 {line} 行：\\u 转义不对"))?;
                    out.push(code);
                }
                _ => return Err(format!("第 {line} 行：不认识的转义")),
            },
            '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    Err(format!("第 {line} 行：双引号没有结束"))
}

fn single_quoted(text: &str, line: usize) -> Result<(String, usize), String> {
    let mut out = String::new();
    let mut chars = text.char_indices().skip(1).peekable();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\'' if chars.peek().map(|(_, c)| *c) == Some('\'') => {
                chars.next();
                out.push('\'');
            }
            '\'' => return Ok((out, index + 1)),
            '\n' => out.push(' '),
            other => out.push(other),
        }
    }
    Err(format!("第 {line} 行：单引号没有结束"))
}

/// One flow-list item: quoted or plain.
fn item(text: &str, line: usize) -> Result<String, String> {
    let text = text.trim();
    if text.starts_with('"') {
        Ok(double_quoted(text, line)?.0)
    } else if text.starts_with('\'') {
        Ok(single_quoted(text, line)?.0)
    } else {
        Ok(plain(text))
    }
}

fn flow_list(text: &str, line: usize) -> Result<Vec<String>, String> {
    let inner = text
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.trim_end().strip_suffix(']'))
        .ok_or(format!("第 {line} 行：列表 [ ] 没有结束"))?;
    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in inner.chars() {
        match (quote, ch) {
            (None, ',') => {
                items.push(std::mem::take(&mut current));
                continue;
            }
            (None, '"' | '\'') => quote = Some(ch),
            (Some(open), _) if ch == open => quote = None,
            _ => {}
        }
        current.push(ch);
    }
    items.push(current);
    items
        .into_iter()
        .filter(|item| !item.trim().is_empty())
        .map(|value| item(&value, line))
        .collect()
}

/// A `|` or `>` block: the following lines indented more than the key.
fn block(indicator: &str, lines: &[&str], line: usize) -> Result<String, String> {
    let style = indicator.chars().next().unwrap_or('|');
    if !indicator[1..].chars().all(|c| matches!(c, '-' | '+' | '1'..='9')) {
        return Err(format!("第 {line} 行：块标记 {indicator} 不对"));
    }
    let width = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| indent(l)).min().unwrap_or(0);
    let text: Vec<String> =
        lines
            .iter()
            .map(|l| {
                if l.trim().is_empty() { String::new() } else { l[width.min(l.len())..].to_owned() }
            })
            .collect();
    let joined = if style == '>' {
        let mut out = String::new();
        for (index, part) in text.iter().enumerate() {
            if part.is_empty() {
                out.push('\n');
            } else {
                if index > 0 && !out.is_empty() && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push_str(part);
            }
        }
        out
    } else {
        text.join("\n")
    };
    Ok(joined.trim_end().to_owned())
}

/// The value of one top-level key: its first-line text and the continuation lines under it.
fn value(first: &str, rest: &[&str], line: usize) -> Result<Value, String> {
    let first = first.trim();
    if first.is_empty() {
        let lines: Vec<&str> = rest.iter().copied().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            return Ok(Value::Text(String::new()));
        }
        if lines.iter().all(|l| l.trim_start().starts_with("- ") || l.trim() == "-") {
            return lines
                .iter()
                .map(|l| item(l.trim_start().trim_start_matches('-'), line))
                .collect::<Result<_, _>>()
                .map(Value::List);
        }
        if lines.iter().any(|l| l.trim_start().starts_with("- ")) {
            return Err(format!("第 {line} 行：列表和映射混在一起"));
        }
        return Ok(Value::Raw);
    }
    if first.starts_with('|') || first.starts_with('>') {
        return block(plain(first).as_str(), rest, line).map(Value::Text);
    }
    if first.starts_with('{') {
        return Ok(Value::Raw);
    }
    let joined = std::iter::once(first).chain(rest.iter().map(|l| l.trim())).collect::<Vec<_>>();
    if first.starts_with('[') {
        return flow_list(&joined.join(" "), line).map(Value::List);
    }
    if first.starts_with('"') || first.starts_with('\'') {
        let text = joined.join("\n");
        let (parsed, end) = if first.starts_with('"') {
            double_quoted(&text, line)?
        } else {
            single_quoted(&text, line)?
        };
        if !plain(&text[end..]).is_empty() {
            return Err(format!("第 {line} 行：引号后面还有内容"));
        }
        return Ok(Value::Text(parsed));
    }
    // A plain scalar may continue on more indented lines; they fold with spaces.
    Ok(Value::Text(
        joined
            .iter()
            .map(|part| plain(part))
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    ))
}

fn text(key: &str, value: Value) -> Result<String, String> {
    match value {
        Value::Text(text) => Ok(text),
        _ => Err(format!("{key} 必须是一段文字")),
    }
}

/// Splits `SKILL.md` and reads its front matter. Problems stop the Skill; warnings do not.
pub fn parse_skill_markdown(source: &str) -> Result<ParsedSkill, Vec<String>> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source).replace("\r\n", "\n");
    let mut lines = source.split('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(vec!["SKILL.md 必须以 --- 开头的 YAML front matter 开始".into()]);
    }
    let lines: Vec<&str> = lines.collect();
    let Some(close) = lines.iter().position(|l| matches!(l.trim_end(), "---" | "...")) else {
        return Err(vec!["front matter 没有用 --- 结束".into()]);
    };
    let (head, body) = (&lines[..close], lines[close + 1..].join("\n"));
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let mut fields: Vec<(String, Value)> = Vec::new();
    let mut index = 0;
    while index < head.len() {
        let line = head[index];
        let number = index + 2;
        index += 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with('\t') {
            problems.push(format!("第 {number} 行：YAML 不能用 Tab 缩进"));
            continue;
        }
        if indent(line) > 0 || line.starts_with("- ") {
            problems.push(format!("第 {number} 行：不在任何键下面"));
            continue;
        }
        let Some((key, first)) = line.split_once(':') else {
            problems.push(format!("第 {number} 行：应为 key: value"));
            continue;
        };
        let key = key.trim().to_owned();
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            problems.push(format!("第 {number} 行：键名 {key} 不对"));
            continue;
        }
        // A list may sit at the key's own indentation, but only under a key with no value.
        let listed = first.trim().is_empty();
        let start = index;
        while index < head.len()
            && (head[index].trim().is_empty()
                || indent(head[index]) > 0
                || listed && head[index].starts_with("- "))
        {
            index += 1;
        }
        let mut rest: Vec<&str> = head[start..index].to_vec();
        while rest.last().is_some_and(|l| l.trim().is_empty()) {
            rest.pop();
        }
        if fields.iter().any(|(existing, _)| *existing == key) {
            problems.push(format!("第 {number} 行：{key} 重复"));
            continue;
        }
        match value(first, &rest, number) {
            Ok(parsed) => fields.push((key, parsed)),
            Err(problem) => problems.push(problem),
        }
    }
    let mut frontmatter = SkillFrontmatter::default();
    for (key, parsed) in fields {
        let result = match key.as_str() {
            "name" => text(&key, parsed).map(|v| frontmatter.name = v.trim().to_owned()),
            "description" => {
                text(&key, parsed).map(|v| frontmatter.description = v.trim().to_owned())
            }
            "license" => {
                text(&key, parsed).map(|v| frontmatter.license = Some(v.trim().to_owned()))
            }
            "compatibility" => {
                text(&key, parsed).map(|v| frontmatter.compatibility = Some(v.trim().to_owned()))
            }
            "allowed-tools" | "allowed_tools" => {
                frontmatter.allowed_tools = match parsed {
                    Value::List(items) => items,
                    Value::Text(text) => text
                        .split([',', ' '])
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    Value::Raw => Vec::new(),
                };
                Ok(())
            }
            _ => {
                frontmatter.other_keys.push(key.clone());
                Ok(())
            }
        };
        if let Err(problem) = result {
            problems.push(problem);
        }
    }
    let name = &frontmatter.name;
    if name.is_empty() {
        problems.push("缺少 name".into());
    } else if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        problems.push(format!("name 最长 {MAX_NAME_CHARS} 个字符，且不能有控制字符"));
    } else if !valid_name(name) {
        warnings.push("name 建议只用小写字母、数字和单个连字符".into());
    }
    let description = &frontmatter.description;
    if description.is_empty() {
        problems.push("缺少 description".into());
    } else if description.chars().count() > MAX_DESCRIPTION_CHARS {
        problems.push(format!("description 最长 {MAX_DESCRIPTION_CHARS} 个字符"));
    }
    if frontmatter
        .compatibility
        .as_ref()
        .is_some_and(|c| c.chars().count() > MAX_COMPATIBILITY_CHARS)
    {
        problems.push(format!("compatibility 最长 {MAX_COMPATIBILITY_CHARS} 个字符"));
    }
    if body.trim().is_empty() {
        problems.push("SKILL.md 正文是空的".into());
    }
    if problems.is_empty() {
        Ok(ParsedSkill { frontmatter, body: body.trim_start_matches('\n').to_owned(), warnings })
    } else {
        Err(problems)
    }
}
