//! What the model reads of a text file `fs.read` returned and of an `fs.search`: one line of JSON
//! with what the result says about the file or the search, then the lines themselves, each after
//! its number (from 1) and a tab. Read as one JSON string, every quote, backslash and line break of
//! the file came escaped and nothing was numbered, and a model copying lines from it into a patch
//! wrote the escapes too. Any other result, a picture `fs.read` returned and a result that did not
//! succeed read as before; the stored result is unchanged.
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::{
    ModelVisibleToolResultEnvelopeV1 as Envelope, ModelVisibleToolResultOutcomeV1 as Outcome,
    ModelVisibleToolSuccessPayloadV1 as Payload,
};
use serde_json::{Map, Value, json};
use std::fmt::Write as _;

/// The text the model reads of `envelope`, the result of tool `name`.
pub(crate) fn model_text(name: &str, envelope: &Envelope) -> String {
    let shown = match (name, succeeded(envelope)) {
        ("fs.read", Some(result)) => read(result, envelope),
        ("fs.search", Some(result)) => search(result, envelope),
        _ => None,
    };
    shown.unwrap_or_else(|| envelope.model_text())
}

/// The structured value of a result that succeeded without an effect.
fn succeeded(envelope: &Envelope) -> Option<&Map<String, Value>> {
    match &envelope.outcome {
        Outcome::Succeeded { payload: Payload::StructuredJson { value } }
            if envelope.side_effect_state == SideEffectStateV1::NoEffect =>
        {
            value.as_object()
        }
        _ => None,
    }
}

/// The first line: `fields` as JSON, with the result's warnings.
fn head(mut fields: Map<String, Value>, envelope: &Envelope) -> String {
    if !envelope.warnings.is_empty() {
        let warnings = serde_json::to_value(&envelope.warnings).unwrap_or_default();
        fields.insert("warnings".to_owned(), warnings);
    }
    Value::Object(fields).to_string()
}

fn flag(result: &Map<String, Value>, field: &str) -> bool {
    result.get(field) == Some(&Value::Bool(true))
}

/// A text file's lines: a header with its path, the lines or characters returned, its hash,
/// encoding and line endings, then the text. A lines range is numbered; a characters range may
/// start inside a line and is shown as it is.
fn read(result: &Map<String, Value>, envelope: &Envelope) -> Option<String> {
    let text = result.get("text")?.as_str()?;
    let range = result.get("returned_range")?.as_object()?;
    // An outside file is named by the full path the model uses for it, beside the authority
    // that let it be read.
    let external = result.get("external").filter(|external| !external.is_null());
    let path = external
        .and_then(|external| external.get("display_path"))
        .filter(|path| path.is_string())
        .or_else(|| result.get("path"))?;
    let first_line = match range.get("range_kind")?.as_str()? {
        "full" => Some(0),
        "lines" => Some(range.get("start_line_index")?.as_u64()?),
        "characters" => None,
        _ => return None,
    };
    let lines = lines(text);
    let mut fields = Map::new();
    fields.insert("path".to_owned(), path.clone());
    if let Some(external) = external {
        fields.insert("external".to_owned(), external.clone());
    }
    match first_line {
        Some(start) => {
            let shown = match u64::try_from(lines.len()).ok()? {
                0 => json!([]),
                count => json!([start + 1, start + count]),
            };
            fields.insert("lines".to_owned(), shown);
            fields.insert("total_lines".to_owned(), result.get("total_lines")?.clone());
        }
        None => {
            let start = range.get("start_character_offset")?.as_u64()?;
            let shown = match u64::try_from(text.chars().count()).ok()? {
                0 => json!([]),
                count => json!([start + 1, start + count]),
            };
            fields.insert("characters".to_owned(), shown);
            fields.insert("total_characters".to_owned(), result.get("total_characters")?.clone());
        }
    }
    for field in ["logical_hash", "encoding", "line_ending"] {
        fields.insert(field.to_owned(), result.get(field)?.clone());
    }
    if flag(result, "is_dirty") {
        fields.insert("unsaved_editor_changes".to_owned(), Value::Bool(true));
    }
    if flag(result, "disk_may_be_stale") {
        fields.insert("disk_may_be_stale".to_owned(), Value::Bool(true));
    }
    let mut shown = head(fields, envelope);
    match first_line {
        Some(start) => {
            for (number, line) in (start + 1..).zip(&lines) {
                let _ = write!(shown, "\n{number}\t{line}");
            }
        }
        None if !text.is_empty() => {
            shown.push('\n');
            shown.push_str(text);
        }
        None => {}
    }
    Some(shown)
}

/// `text`'s lines without their line endings; a final line break ends the last line.
fn lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect()
}

/// A search: a header with how it went, then for each file a line of JSON with its path and hash,
/// followed by its matching lines. A line matched more than once is shown once. Lines around the
/// matches, when the call asked for them, come in their place among them, a `-` after the number.
fn search(result: &Map<String, Value>, envelope: &Envelope) -> Option<String> {
    let matches = result.get("matches")?.as_array()?;
    let context = result.get("context").and_then(Value::as_array);
    let mut fields = result.clone();
    fields.remove("matches");
    fields.remove("context");
    fields.retain(|_, value| !value.is_null());
    fields.insert("match_count".to_owned(), json!(matches.len()));
    let mut shown = head(fields, envelope);
    let mut file = None;
    let mut last = None;
    let mut around: &[Value] = &[];
    for found in matches {
        let found = found.as_object()?;
        let path = found.get("path")?.as_str()?;
        if file != Some(path) {
            write_around(&mut shown, &mut around, u64::MAX)?;
            let mut about = Map::new();
            about.insert("path".to_owned(), json!(path));
            about.insert("logical_hash".to_owned(), found.get("logical_hash")?.clone());
            if let Some(source) =
                found.get("source").filter(|source| source.as_str() != Some("disk"))
            {
                about.insert("source".to_owned(), source.clone());
            }
            if let Some(revision) = found.get("buffer_revision").filter(|value| !value.is_null()) {
                about.insert("buffer_revision".to_owned(), revision.clone());
            }
            for field in ["ignored", "generated"] {
                if flag(found, field) {
                    about.insert(field.to_owned(), Value::Bool(true));
                }
            }
            shown.push('\n');
            shown.push_str(&Value::Object(about).to_string());
            file = Some(path);
            last = None;
            around = context_of(context, path);
        }
        let line = found.get("line_index")?.as_u64()? + 1;
        write_around(&mut shown, &mut around, line)?;
        let excerpt = found.get("excerpt")?.as_str()?;
        let excerpt = excerpt.strip_suffix('\r').unwrap_or(excerpt);
        if last == Some((line, excerpt)) {
            continue;
        }
        last = Some((line, excerpt));
        let before = if flag(found, "excerpt_truncated_before") { "…" } else { "" };
        let after = if flag(found, "excerpt_truncated_after") { "…" } else { "" };
        let _ = write!(shown, "\n{line}\t{before}{excerpt}{after}");
    }
    write_around(&mut shown, &mut around, u64::MAX)?;
    Some(shown)
}

/// The lines around the matches in `path`, in order, when the call asked for them.
fn context_of<'a>(context: Option<&'a Vec<Value>>, path: &str) -> &'a [Value] {
    context
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .find(|file| file.get("path").and_then(Value::as_str) == Some(path))
        .and_then(|file| file.get("lines"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

/// Writes the lines of `pending` numbered below `before`, and leaves the others in it.
fn write_around(shown: &mut String, pending: &mut &[Value], before: u64) -> Option<()> {
    while let Some((line, rest)) = pending.split_first() {
        let line = line.as_object()?;
        let number = line.get("line_index")?.as_u64()? + 1;
        if number >= before {
            break;
        }
        let text = line.get("text")?.as_str()?;
        let cut = if flag(line, "truncated") { "…" } else { "" };
        let _ = write!(shown, "\n{number}-\t{text}{cut}");
        *pending = rest;
    }
    Some(())
}

#[cfg(test)]
#[path = "result_text_tests.rs"]
mod tests;
