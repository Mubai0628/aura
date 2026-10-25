//! What the model reads of a tool result. The stored envelope keeps
//! every field for audit and replay. The model reads only what bears on its next step - the
//! outcome, the side-effect state, warnings and the artifacts it may read. The call id, the tool
//! definition with its registry snapshot, the completion time and each artifact's hash were noise
//! to it, and the two hashes were the only ones a write result showed.
use serde_json::{Map, Value, json};

use crate::ModelVisibleToolResultEnvelopeV1;

impl ModelVisibleToolResultEnvelopeV1 {
    /// The result as the model reads it.
    #[must_use]
    pub fn model_view(&self) -> Value {
        let mut view = Map::new();
        view.insert("outcome".to_owned(), serde_json::to_value(&self.outcome).unwrap_or_default());
        view.insert(
            "side_effect_state".to_owned(),
            serde_json::to_value(self.side_effect_state).unwrap_or_default(),
        );
        if !self.warnings.is_empty() {
            view.insert(
                "warnings".to_owned(),
                serde_json::to_value(&self.warnings).unwrap_or_default(),
            );
        }
        if !self.artifacts.is_empty() {
            let artifacts = self
                .artifacts
                .iter()
                .map(|artifact| {
                    json!({
                        "artifact_id": artifact.artifact_id,
                        "slot": artifact.slot,
                        "media_type": artifact.media_type,
                        "byte_length": artifact.byte_length,
                    })
                })
                .collect();
            view.insert("artifacts".to_owned(), Value::Array(artifacts));
        }
        Value::Object(view)
    }

    /// [`Self::model_view`] as the text a tool result message carries.
    #[must_use]
    pub fn model_text(&self) -> String {
        self.model_view().to_string()
    }
}

#[cfg(test)]
#[path = "model_view_tests.rs"]
mod tests;
