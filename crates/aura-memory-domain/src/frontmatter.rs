//! Deliberately flat YAML: scalar fields and scalar sequences, no tags, aliases,
//! objects or executable YAML types. JSON quoting is valid YAML and is used on write.
use crate::{MAX_NOTE_BYTES, MemoryDocument, MemoryFormatError, MemoryMetadata};
use serde_json::{Map, Value};

const ARRAYS: &[&str] = &[
    "source_sessions",
    "source_events",
    "tags",
    "related_ids",
    "supersedes_ids",
    "conflicts_with_ids",
];
const NUMBERS: &[&str] = &["schema_version", "confidence", "importance", "recall_weight"];
/// Aura's schema, in the stable order it writes. Every other flat property is
/// the user's (Obsidian `aliases`, `cssclasses`, their own fields) and is kept.
const SCHEMA: &[&str] = &[
    "schema_version",
    "id",
    "scope",
    "category",
    "status",
    "confidence",
    "importance",
    "recall_weight",
    "created_at",
    "updated_at",
    "last_confirmed_at",
    "source_sessions",
    "source_events",
    "tags",
    "related_ids",
    "supersedes_ids",
    "conflicts_with_ids",
];

/// Who the next `- item` line belongs to.
enum Items {
    None,
    Schema(String),
    User(usize),
}

pub fn parse(text: &str) -> Result<MemoryDocument, MemoryFormatError> {
    let (fields, body, user) = parse_fields(text)?;
    let mut metadata: MemoryMetadata = serde_json::from_value(Value::Object(fields))
        .map_err(|_| MemoryFormatError::Invalid("schema"))?;
    metadata.validate()?;
    metadata.user_properties = user;
    Ok(MemoryDocument { metadata, body })
}

type Fields = (Map<String, Value>, String, Vec<String>);

fn parse_fields(text: &str) -> Result<Fields, MemoryFormatError> {
    if text.len() > MAX_NOTE_BYTES {
        return Err(MemoryFormatError::TooLarge);
    }
    let normalized = text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n");
    let mut lines = normalized.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(MemoryFormatError::Frontmatter(1));
    }
    let mut fields = Map::new();
    let mut user: Vec<String> = vec![];
    let mut user_keys = std::collections::BTreeSet::new();
    let mut items = Items::None;
    let mut body_at = normalized.find('\n').map_or(normalized.len(), |i| i + 1);
    let mut ended = false;
    for (index, line) in lines.enumerate() {
        body_at += line.len();
        let raw = line.trim_end();
        if raw == "---" {
            ended = true;
            break;
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            let item = scalar(item, index + 2)?;
            match &items {
                Items::Schema(key) => fields
                    .get_mut(key)
                    .and_then(Value::as_array_mut)
                    .ok_or(MemoryFormatError::Frontmatter(index + 2))?
                    .push(Value::String(item)),
                Items::User(at) => {
                    user[*at].push('\n');
                    user[*at].push_str(raw);
                }
                Items::None => return Err(MemoryFormatError::Frontmatter(index + 2)),
            }
            continue;
        }
        if raw.starts_with(char::is_whitespace) {
            return Err(MemoryFormatError::Frontmatter(index + 2));
        }
        let (key, value) = raw.split_once(':').ok_or(MemoryFormatError::Frontmatter(index + 2))?;
        if fields.contains_key(key) || user_keys.contains(key) {
            return Err(MemoryFormatError::Frontmatter(index + 2));
        }
        let value = strip_comment(value).trim();
        items = Items::None;
        if !SCHEMA.contains(&key) {
            // A user property stays exactly as written, but only if it is flat:
            // a scalar, a flow list, or a block list on the following lines.
            if !user_key(key) {
                return Err(MemoryFormatError::Frontmatter(index + 2));
            }
            if value.is_empty() {
                items = Items::User(user.len());
            } else if value.starts_with('[') {
                sequence(value, index + 2)?;
            } else {
                scalar(value, index + 2)?;
            }
            user_keys.insert(key.to_owned());
            user.push(raw.to_owned());
            continue;
        }
        let parsed = if ARRAYS.contains(&key) {
            items = Items::Schema(key.to_owned());
            Value::Array(sequence(value, index + 2)?.into_iter().map(Value::String).collect())
        } else if NUMBERS.contains(&key) {
            let number: Value = serde_json::from_str(value)
                .map_err(|_| MemoryFormatError::Frontmatter(index + 2))?;
            if !number.is_number() {
                return Err(MemoryFormatError::Frontmatter(index + 2));
            }
            number
        } else if key == "last_confirmed_at" && matches!(value, "null" | "~" | "") {
            Value::Null
        } else {
            Value::String(scalar(value, index + 2)?)
        };
        fields.insert(key.to_owned(), parsed);
    }
    if !ended {
        return Err(MemoryFormatError::Invalid("unterminated frontmatter"));
    }
    Ok((fields, normalized[body_at..].to_owned(), user))
}

/// Property names as Obsidian writes them, never a YAML indicator or a nested key.
fn user_key(key: &str) -> bool {
    const INDICATORS: &[char] = &[
        '-', '?', '[', ']', '{', '}', ',', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
    ];
    !key.is_empty()
        && key.len() <= 128
        && key == key.trim()
        && !key.starts_with(INDICATORS)
        && !key.chars().any(char::is_control)
}

/// Only missing defaults are supplied. Invalid existing values, unknown fields
/// and malformed YAML remain manual work; the Markdown body is never rewritten.
pub fn repair_missing_fields(
    text: &str,
    scope: crate::MemoryScope,
    category: &str,
) -> Result<Option<(MemoryDocument, Vec<String>)>, MemoryFormatError> {
    let (mut fields, body, user) = if text.trim_start_matches('\u{feff}').starts_with("---") {
        parse_fields(text)?
    } else {
        (Map::new(), text.to_owned(), vec![])
    };
    let Value::Object(defaults) = serde_json::to_value(MemoryMetadata::new(scope, category.into()))
        .map_err(|_| MemoryFormatError::Invalid("schema"))?
    else {
        return Err(MemoryFormatError::Invalid("schema"));
    };
    let mut changes = vec![];
    for (key, value) in defaults {
        if !fields.contains_key(&key) {
            changes.push(format!("补全 {key}"));
            fields.insert(key, value);
        }
    }
    if changes.is_empty() {
        return Ok(None);
    }
    let mut metadata: MemoryMetadata = serde_json::from_value(Value::Object(fields))
        .map_err(|_| MemoryFormatError::Invalid("existing field requires manual repair"))?;
    metadata.validate()?;
    metadata.user_properties = user;
    if metadata.scope != scope {
        return Err(MemoryFormatError::Invalid("scope"));
    }
    Ok(Some((MemoryDocument { metadata, body }, changes)))
}

pub fn render(note: &MemoryDocument) -> Result<String, MemoryFormatError> {
    note.metadata.validate()?;
    let Value::Object(fields) =
        serde_json::to_value(&note.metadata).map_err(|_| MemoryFormatError::Invalid("schema"))?
    else {
        return Err(MemoryFormatError::Invalid("schema"));
    };
    let mut text = String::from("---\n");
    // Stable field order is canonical for Aura, but never the identity of a note.
    for &key in SCHEMA {
        text.push_str(key);
        text.push_str(": ");
        text.push_str(&fields[key].to_string());
        text.push('\n');
    }
    for property in &note.metadata.user_properties {
        text.push_str(property);
        text.push('\n');
    }
    text.push_str("---\n");
    text.push_str(&note.body);
    if text.len() > MAX_NOTE_BYTES {
        return Err(MemoryFormatError::TooLarge);
    }
    Ok(text)
}

fn scalar(value: &str, line: usize) -> Result<String, MemoryFormatError> {
    let value = strip_comment(value).trim();
    if value.starts_with('"') {
        return serde_json::from_str::<String>(value)
            .map_err(|_| MemoryFormatError::Frontmatter(line));
    }
    if let Some(inner) = value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Ok(inner.replace("''", "'"));
    }
    if value.is_empty()
        || value.starts_with(['&', '*', '!', '{', '[', '|', '>', '\''])
        || value.contains('\0')
    {
        return Err(MemoryFormatError::Frontmatter(line));
    }
    Ok(value.to_owned())
}

fn sequence(value: &str, line: usize) -> Result<Vec<String>, MemoryFormatError> {
    if value.is_empty() {
        return Ok(vec![]);
    }
    let inner = value
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or(MemoryFormatError::Frontmatter(line))?;
    if inner.trim().is_empty() {
        return Ok(vec![]);
    }
    let mut result = vec![];
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c == ',' {
            result.push(scalar(inner[start..i].trim(), line)?);
            start = i + 1;
        }
    }
    if quote.is_some() {
        return Err(MemoryFormatError::Frontmatter(line));
    }
    result.push(scalar(inner[start..].trim(), line)?);
    Ok(result)
}

fn strip_comment(value: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c == '#' && (i == 0 || value[..i].ends_with(char::is_whitespace)) {
            return &value[..i];
        }
    }
    value
}
