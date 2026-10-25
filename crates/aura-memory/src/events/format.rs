use super::*;
use serde::{Deserialize, Serialize};
const MARKER: &str = "<!-- aura-event ";

pub(super) struct Block {
    pub id: String,
    pub at: UtcTimestamp,
    pub text: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    id: String,
    at: UtcTimestamp,
    session_id: Option<SessionId>,
}

pub(super) fn render_block(event: &ProjectEvent) -> MemoryResult<Block> {
    let header = Header { id: event.id.clone(), at: event.at, session_id: event.session_id };
    let json =
        serde_json::to_string(&header).map_err(|_| MemoryError::Invalid("event metadata"))?;
    Ok(Block {
        id: event.id.clone(),
        at: event.at,
        text: format!(
            "{MARKER}{json} -->\n## {} · {}\n\n{}\n\n",
            event.at,
            event.title,
            event.summary.trim()
        ),
    })
}
pub(super) fn parse_blocks(body: &str) -> MemoryResult<(String, Vec<Block>)> {
    let mut starts = vec![];
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        if line.starts_with(MARKER) {
            starts.push(offset);
        }
        offset += line.len();
    }
    let prefix = body[..starts.first().copied().unwrap_or(body.len())].to_string();
    let mut blocks = vec![];
    for (i, start) in starts.iter().copied().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(body.len());
        let text = &body[start..end];
        let first = text.lines().next().ok_or(MemoryError::Invalid("event marker"))?;
        let raw = first
            .strip_prefix(MARKER)
            .and_then(|s| s.strip_suffix(" -->"))
            .ok_or(MemoryError::Invalid("event marker changed; repair manually"))?;
        let header: Header = serde_json::from_str(raw)
            .map_err(|_| MemoryError::Invalid("event metadata changed; repair manually"))?;
        if blocks.iter().any(|b: &Block| b.id == header.id) {
            return Err(MemoryError::Invalid("duplicate event identity"));
        }
        blocks.push(Block { id: header.id, at: header.at, text: text.into() });
    }
    Ok((prefix, blocks))
}
