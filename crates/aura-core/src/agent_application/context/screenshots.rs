//! The newest screenshots of the built-in browser and of the user's Chrome
//! go to a model that takes images, each with the tool result it belongs to. They are read once
//! per step; before each compilation pass only those whose results are still in the history, not
//! cleared and not compacted away, are attached. A screenshot travels as its tool's result, on the
//! same basis and in the same class as the result's text.
//!
//! So do the pictures the user sent with a
//! message, each with its message, as the user's own submission.
//! The latest media.frames result carries its frames or contact sheet on the same basis.
use super::*;
use aura_agent_domain::{MAX_MEDIA_FRAMES, MediaFramesResult};
use aura_model_domain::{ModelAttachmentSnapshot, ResolvedModelAttachment};
use aura_tool_contract::{ModelVisibleToolResultOutcomeV1, ModelVisibleToolSuccessPayloadV1};
#[cfg(test)]
#[path = "screenshots_tests.rs"]
mod tests;

/// How many of the newest screenshots a request carries.
const SENT: usize = 3;
/// How many of the newest pictures sent with user messages a request carries.
const PICTURES: usize = 20;

/// What an image goes with in the request.
enum Place {
    /// The tool call's result that names it.
    Result,
    /// The user message it was sent with.
    Message,
}

struct Shot {
    result_event: EventId,
    place: Place,
    attachment: ResolvedModelAttachment,
}

#[derive(Default)]
pub(in crate::agent_application::context) struct Screenshots(Vec<Shot>);

/// The screenshot a browser result names, when it said the model would see it.
fn shown_screenshot(value: &serde_json::Value) -> Option<ArtifactId> {
    match value {
        serde_json::Value::Object(fields) => {
            if fields.get("shown_to_model") == Some(&serde_json::Value::Bool(true))
                && let Some(id) = fields.get("image_artifact_id").and_then(|id| id.as_str())
            {
                return id.parse().ok();
            }
            fields.values().find_map(shown_screenshot)
        }
        serde_json::Value::Array(values) => values.iter().find_map(shown_screenshot),
        _ => None,
    }
}

struct ToolImage {
    result_event: EventId,
    artifact_id: ArtifactId,
    display_name: String,
}

/// History carries the envelope's model view, not its raw structured result. Only the typed
/// successful payload at this exact position is authoritative; artifact metadata is not payload.
fn media_result(content: &str) -> Option<MediaFramesResult> {
    let mut view: serde_json::Value = serde_json::from_str(content).ok()?;
    let outcome: ModelVisibleToolResultOutcomeV1 =
        serde_json::from_value(view.get_mut("outcome")?.take()).ok()?;
    let ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
    } = outcome
    else {
        return None;
    };
    serde_json::from_value(value).ok()
}

/// Select whole result groups before reversing them: frames keep their requested order. A newer
/// media result replaces the previous one even when it failed, is hidden, or has no images.
fn tool_images(items: &[LogicalConversationItem]) -> Vec<ToolImage> {
    let mut groups = Vec::new();
    let mut screenshots = 0;
    let mut media_seen = false;
    for item in items.iter().rev() {
        if screenshots == SENT && media_seen {
            break;
        }
        let LogicalConversationItem::ToolResult {
            source_event_id,
            tool_name,
            content,
            is_error,
            ..
        } = item
        else {
            continue;
        };
        let mut images = Vec::new();
        if tool_name == "media.frames" && !media_seen {
            media_seen = true;
            if *is_error {
                continue;
            }
            if let Some(result) = media_result(content)
                && result.shown_to_model
            {
                if let Some(id) = result.sheet_artifact_id {
                    images.push((id, "video contact sheet".to_owned()));
                } else {
                    images.extend(result.frames.into_iter().take(MAX_MEDIA_FRAMES).filter_map(
                        |frame| {
                            frame
                                .image_artifact_id
                                .map(|id| (id, format!("video frame at {:.3}s", frame.actual_time)))
                        },
                    ));
                }
            }
        } else if matches!(tool_name.as_str(), "browser.screenshot" | "chrome.screenshot")
            && !*is_error
            && screenshots < SENT
            && let Some(id) = serde_json::from_str::<serde_json::Value>(content)
                .ok()
                .as_ref()
                .and_then(shown_screenshot)
        {
            screenshots += 1;
            let shown = if tool_name == "chrome.screenshot" { "Chrome" } else { "browser" };
            images.push((id, format!("{shown} screenshot")));
        }
        if !images.is_empty() {
            groups.push(
                images
                    .into_iter()
                    .map(|(artifact_id, display_name)| ToolImage {
                        result_event: *source_event_id,
                        artifact_id,
                        display_name,
                    })
                    .collect::<Vec<_>>(),
            );
        }
    }
    groups.into_iter().rev().flatten().collect()
}

impl AgentApplication {
    /// The newest screenshots of the history, read from the conversation's store; none for a model
    /// that is not sent images.
    pub(super) async fn screenshots(
        self: &Arc<Self>,
        run_id: AgentRunId,
        session: SessionId,
        items: &[LogicalConversationItem],
        current: &[EventId],
        takes_images: bool,
    ) -> anyhow::Result<Screenshots> {
        if !takes_images {
            return Ok(Screenshots::default());
        }
        // The user messages of the history and the step's own, newest first.
        let mut messages = items
            .iter()
            .filter_map(|item| match item {
                LogicalConversationItem::User { source_event_id, .. } => Some(*source_event_id),
                _ => None,
            })
            .chain(current.iter().copied())
            .collect::<Vec<_>>();
        messages.reverse();
        let wanted = tool_images(items);
        if wanted.is_empty() && messages.is_empty() {
            return Ok(Screenshots::default());
        }
        let journal = Arc::clone(&self.execution.journal);
        let read = self
            .store
            .run_critical(
                StoreStep::run(run_id, "agent.context.screenshots"),
                StoreAccess::Read,
                move || {
                    let journal = journal.lock()?;
                    let db = journal.database.connection();
                    // A screenshot this conversation cannot read (one a branch carried over) is
                    // left out, not a failure of the step.
                    let shots = wanted
                        .into_iter()
                        .filter_map(|image| {
                            artifact::read(db, image.artifact_id, &session.to_string())
                                .ok()
                                .map(|bytes| (image, bytes))
                        })
                        .collect::<Vec<_>>();
                    // The newest pictures sent with the messages, newest first; a picture this
                    // conversation cannot read is left out too.
                    let mut pictures = Vec::new();
                    for message in messages {
                        let Some((_, attached)) =
                            aura_persistence_v2::agent::attachments::of_message(db, message)?
                        else {
                            continue;
                        };
                        for attachment in attached.iter().rev() {
                            if pictures.len() == PICTURES {
                                break;
                            }
                            if let AgentMessageAttachment::Image { name, artifact_id, .. } =
                                attachment
                                && let Ok(bytes) =
                                    artifact::read(db, *artifact_id, &session.to_string())
                            {
                                pictures.push((message, *artifact_id, name.clone(), bytes));
                            }
                        }
                    }
                    Ok((shots, pictures))
                },
            )
            .await?;
        let (read, pictures) = read;
        let mut shots = read
            .into_iter()
            .filter_map(|(image, bytes)| {
                let media_type =
                    crate::agent_application::browser_screenshot::image_media_type(&bytes)?;
                Some(Shot {
                    result_event: image.result_event,
                    place: Place::Result,
                    attachment: ResolvedModelAttachment {
                        snapshot: ModelAttachmentSnapshot {
                            source_event_id: image.result_event,
                            artifact_id: image.artifact_id,
                            file_reference_id: None,
                            display_name: image.display_name,
                            media_type: media_type.into(),
                            byte_length: bytes.len() as u64,
                            content_hash: ContentHash::sha256(&bytes),
                        },
                        bytes: bytes.into(),
                    },
                })
            })
            .collect::<Vec<_>>();
        // In the order they were sent.
        shots.extend(pictures.into_iter().rev().filter_map(|(message, id, name, bytes)| {
            let media_type =
                crate::agent_application::browser_screenshot::image_media_type(&bytes)?;
            Some(Shot {
                result_event: message,
                place: Place::Message,
                attachment: ResolvedModelAttachment {
                    snapshot: ModelAttachmentSnapshot {
                        source_event_id: message,
                        artifact_id: id,
                        file_reference_id: None,
                        display_name: name,
                        media_type: media_type.into(),
                        byte_length: bytes.len() as u64,
                        content_hash: ContentHash::sha256(&bytes),
                    },
                    bytes: bytes.into(),
                },
            })
        }));
        Ok(Screenshots(shots))
    }
}

impl Screenshots {
    /// Attaches the screenshots whose results the request still carries as they were.
    pub(in crate::agent_application::context) fn attach(
        &self,
        input: &mut ContextPreparationInput,
    ) {
        // The step's own messages, which the request adds after the history.
        let current = current_messages(&input.trigger);
        let retained = tool_images(&input.recent_history_items);
        let kept = self
            .0
            .iter()
            .filter(|shot| match &shot.place {
                Place::Result => retained.iter().any(|image| {
                    image.result_event == shot.result_event
                        && image.artifact_id == shot.attachment.snapshot.artifact_id
                }),
                Place::Message => {
                    current.contains(&shot.result_event)
                        || input.recent_history_items.iter().any(|item| {
                            matches!(item, LogicalConversationItem::User { source_event_id, .. }
                                if *source_event_id == shot.result_event)
                        })
                }
            })
            .collect::<Vec<_>>();
        input.attachments = kept.iter().map(|shot| shot.attachment.snapshot.clone()).collect();
        input.attachment_egress_sources = kept
            .iter()
            .map(|shot| {
                let snapshot = &shot.attachment.snapshot;
                let (source_kind, data_class, authorization_basis) = match shot.place {
                    Place::Result => (
                        ModelEgressSourceKind::ToolResult,
                        NetworkDataClass::WorkspaceSource,
                        ModelEgressAuthorizationBasis::ModelChannelPolicy,
                    ),
                    // Sent by the user with the message: their own submission.
                    Place::Message => (
                        ModelEgressSourceKind::UserEvent,
                        NetworkDataClass::UserFileContent,
                        ModelEgressAuthorizationBasis::UserSubmission,
                    ),
                };
                ModelEgressSourceDraft {
                    source_ref: ContextSourceRef::Artifact { artifact_id: snapshot.artifact_id },
                    source_kind,
                    data_class,
                    content_hash: snapshot.content_hash.clone(),
                    byte_length: snapshot.byte_length,
                    authorization_basis,
                }
            })
            .collect();
    }

    /// The bytes of the request's attachments, in its order.
    pub(super) fn resolved(
        &self,
        attachments: &[ModelAttachmentSnapshot],
    ) -> anyhow::Result<Vec<ResolvedModelAttachment>> {
        attachments
            .iter()
            .map(|snapshot| {
                self.0
                    .iter()
                    .find(|shot| shot.attachment.snapshot == *snapshot)
                    .map(|shot| shot.attachment.clone())
                    .ok_or_else(|| anyhow::anyhow!("context.attachment_unresolved"))
            })
            .collect()
    }
}

/// The user messages a step itself sends, after the history.
pub(in crate::agent_application::context) fn current_messages(
    trigger: &ContextPreparationTriggerV1,
) -> Vec<EventId> {
    match trigger {
        ContextPreparationTriggerV1::UserSubmission { user_event_id } => vec![*user_event_id],
        ContextPreparationTriggerV1::UserBatch { user_event_ids } => user_event_ids.clone(),
        _ => Vec::new(),
    }
}
