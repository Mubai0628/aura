//! The images a request carries on its tool
//! results, such as the built-in browser's screenshots. Each goes with the tool result its source
//! event names; a request that carries an image anywhere else, or something that is not an image,
//! is refused rather than sent some other way. A picture the user sent with a message goes
//! with that message, after its text.
use crate::ModelAdapterError;
use aura_domain_common::EventId;
use aura_model_domain::{
    LogicalConversationItem, LogicalModelRequest, ResolvedModelAttachment, is_model_image,
};
use base64::Engine as _;
use std::collections::{BTreeMap, BTreeSet};

/// One image as the wire formats carry it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultImage {
    pub display_name: String,
    pub media_type: String,
    pub data_base64: String,
}

impl ToolResultImage {
    /// Introduce each picture at its actual position in the user's message.
    #[must_use]
    pub fn user_image_label(&self) -> String {
        format!("Picture `{}` attached by the user:", self.display_name)
    }

    /// The image as a `data:` address, the form the OpenAI formats take.
    #[must_use]
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.data_base64)
    }
}

/// The request's images by the event of the tool result or user message each belongs to, in
/// request order.
pub fn tool_result_images(
    request: &LogicalModelRequest,
    resolved: &[ResolvedModelAttachment],
) -> Result<BTreeMap<EventId, Vec<ToolResultImage>>, ModelAdapterError> {
    let results = request
        .conversation_items
        .iter()
        .filter_map(|item| match item {
            LogicalConversationItem::ToolResult { source_event_id, .. }
            | LogicalConversationItem::User { source_event_id, .. } => Some(*source_event_id),
            LogicalConversationItem::Assistant { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let mut images = BTreeMap::<EventId, Vec<ToolResultImage>>::new();
    for attachment in resolved {
        let snapshot = &attachment.snapshot;
        if !is_model_image(&snapshot.media_type) {
            return Err(ModelAdapterError::invalid_request(
                "model_attachment.media_unsupported",
                "only images are sent to a model with a request",
            ));
        }
        if !results.contains(&snapshot.source_event_id) {
            return Err(ModelAdapterError::invalid_request(
                "model_attachment.unplaced",
                "an image is sent only with the tool result or user message it belongs to",
            ));
        }
        images.entry(snapshot.source_event_id).or_default().push(ToolResultImage {
            display_name: snapshot.display_name.clone(),
            media_type: snapshot.media_type.trim().to_ascii_lowercase(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(&attachment.bytes),
        });
    }
    Ok(images)
}

/// The words that introduce a tool result's image where the format cannot put it in the result.
#[must_use]
pub fn tool_result_image_label(tool_name: &str, call_id: &str) -> String {
    format!("The image {tool_name} returned for call {call_id}:")
}
