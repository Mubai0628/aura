//! MCP resources: the data a server offers by address, listed with `resources/list` and read with
//! `resources/read`. Aura reads one only when the user refers to it in a message; its text is
//! kept, anything binary is described, never decoded.
use crate::protocol::{ProtocolError, text};
use serde_json::Value;

/// One resource as the server listed it.
#[derive(Clone, Debug, PartialEq)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResourcesPage {
    pub resources: Vec<McpResource>,
    pub next_cursor: Option<String>,
}

pub fn parse_resources_page(value: &Value) -> Result<ResourcesPage, ProtocolError> {
    let listed = value
        .get("resources")
        .and_then(Value::as_array)
        .ok_or(ProtocolError::Missing("resources"))?;
    let mut resources = Vec::with_capacity(listed.len());
    for resource in listed {
        let (Some(uri), Some(name)) = (text(resource, "uri", 2048), text(resource, "name", 256))
        else {
            return Err(ProtocolError::Shape("一个资源没有 uri 或 name".into()));
        };
        resources.push(McpResource {
            uri,
            name,
            title: text(resource, "title", 256),
            description: text(resource, "description", 1024),
            mime_type: text(resource, "mimeType", 128),
        });
    }
    Ok(ResourcesPage {
        resources,
        next_cursor: value
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_owned),
    })
}

/// The bytes a standard base64 text decodes to, read from its length.
fn decoded_len(data: &str) -> usize {
    let padding = data.bytes().rev().take_while(|byte| *byte == b'=').count().min(2);
    (data.len() / 4 * 3).saturating_sub(padding)
}

/// One part of a read resource: its text, or what its binary data was and how big.
#[derive(Clone, Debug, PartialEq)]
pub enum ResourcePart {
    Text { uri: String, mime_type: Option<String>, text: String },
    Blob { uri: String, mime_type: Option<String>, bytes: usize },
}

pub fn parse_read_result(value: &Value) -> Result<Vec<ResourcePart>, ProtocolError> {
    let contents = value
        .get("contents")
        .and_then(Value::as_array)
        .ok_or(ProtocolError::Missing("contents"))?;
    Ok(contents
        .iter()
        .map(|part| {
            let uri = text(part, "uri", 2048).unwrap_or_default();
            let mime_type = text(part, "mimeType", 128);
            match part.get("text").and_then(Value::as_str) {
                Some(body) => ResourcePart::Text { uri, mime_type, text: body.to_owned() },
                None => ResourcePart::Blob {
                    uri,
                    mime_type,
                    bytes: part.get("blob").and_then(Value::as_str).map_or(0, decoded_len),
                },
            }
        })
        .collect())
}

/// A read resource as one text of at most `limit` bytes: each part's text, a binary part
/// described on a line of its own. Says whether it was cut.
#[must_use]
pub fn resource_text(parts: &[ResourcePart], limit: usize) -> (String, bool) {
    let joined = parts
        .iter()
        .map(|part| match part {
            ResourcePart::Text { text, .. } => text.clone(),
            ResourcePart::Blob { uri, mime_type, bytes } => format!(
                "[{uri}: binary {}, about {bytes} bytes, not included]",
                mime_type.as_deref().unwrap_or("data")
            ),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if joined.len() <= limit {
        return (joined, false);
    }
    let mut end = limit;
    while !joined.is_char_boundary(end) {
        end -= 1;
    }
    (joined[..end].to_owned(), true)
}
