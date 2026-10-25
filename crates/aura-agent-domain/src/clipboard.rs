//! What the
//! user copies — a reply, a message, a path, a link — is written to the clipboard by Main. Main
//! refuses the browser's clipboard permissions to every page, so the renderer asks Main for exactly
//! this one thing; nothing a model calls reaches it.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Text the user asked to copy.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClipboardWriteRequest {
    /// At most 16 Mi characters; Main also refuses more than 16 MiB as UTF-8.
    #[schemars(length(max = 16_777_216))]
    pub text: String,
}

/// Whether the clipboard took the text, and what the user is told when it did not.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClipboardWritten {
    pub completed: bool,
    pub message: String,
}
