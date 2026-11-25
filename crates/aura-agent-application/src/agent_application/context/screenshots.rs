//! The newest screenshots of the built-in browser, of the user's Chrome and of the user's desktop
//! go to a model that takes images, each with the tool result it belongs to. They are read once
//! per step; before each compilation pass only those whose results are still in the history, not
//! cleared and not compacted away, are attached. A screenshot travels as its tool's result, on the
//! same basis and in the same class as the result's text.
//!
//! Materialized attachments keep their original source binding until the associated history is
//! replaced by a checkpoint, or an explicit request budget recovery defers the image.
//!
//! So do the pictures the user sent with a
//! message, each with its message, as the user's own submission.
//! The latest media.frames result carries its frames or contact sheet on the same basis.
use super::*;
use aura_agent_domain::{MAX_MEDIA_FRAMES, MediaFramesResult};
use aura_model_domain::{ModelAttachmentSnapshot, ResolvedModelAttachment};
use aura_tool_contract::{ModelVisibleToolResultOutcomeV1, ModelVisibleToolSuccessPayloadV1};
use std::collections::BTreeSet;

/// How many of the newest screenshots a request carries at least.
const SENT: usize = 3;
/// How many older screenshots leave the request together.
const BATCH: usize = 3;

/// How many of `count` screenshots, the newest, a request carries: the newest [`SENT`], and the
/// older ones until [`BATCH`] more have come.
fn carried(count: usize) -> usize {
    if count <= SENT { count } else { SENT + (count - SENT) % BATCH }
}
/// How many of the newest pictures sent with user messages a request carries.
const PICTURES: usize = 20;

/// What an image goes with in the request.
enum Place {
    /// The tool call's result that names it.
    Result,
    /// The user message it was sent with.
    Message,
}

pub struct Shot {
    result_event: EventId,
    place: Place,
    attachment: ResolvedModelAttachment,
}

#[derive(Default)]
pub struct Screenshots {
    shots: Vec<Shot>,
    /// The pictures sent with a message that could not be read, each with its message: the model
    /// is told they were not sent, so it does not take them as seen.
    unread: Vec<(EventId, String)>,
}

impl Screenshots {
    /// Whether there is no picture to carry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shots.is_empty()
    }
}

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

pub struct ToolImage {
    result_event: EventId,
    pub artifact_id: ArtifactId,
    display_name: String,
}

struct MediaPrefix {
    events: BTreeSet<EventId>,
    attachments: BTreeSet<(EventId, ArtifactId)>,
}

impl MediaPrefix {
    fn new(snapshot: &ContextProjectionSnapshot) -> Self {
        Self {
            events: snapshot
                .conversation_items
                .iter()
                .map(|i| match i {
                    LogicalConversationItem::User { source_event_id, .. }
                    | LogicalConversationItem::Assistant { source_event_id, .. }
                    | LogicalConversationItem::ToolResult { source_event_id, .. } => {
                        *source_event_id
                    }
                })
                .collect(),
            attachments: snapshot
                .attachments
                .iter()
                .map(|a| (a.source_event_id, a.artifact_id))
                .collect(),
        }
    }

    fn admits(&self, event: EventId, artifact: ArtifactId) -> bool {
        !self.events.contains(&event) || self.attachments.contains(&(event, artifact))
    }
}

/// History carries the envelope's model view, not its raw structured result. Only the typed
/// successful payload at this exact position is authoritative; artifact metadata is not payload.
pub fn media_result(content: &str) -> Option<MediaFramesResult> {
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
/// media result replaces the previous one even when it failed, is hidden, or has no images. Every
/// screenshot the model was shown counts, so how many are carried follows [`carried`].
pub fn tool_images(items: &[LogicalConversationItem]) -> Vec<ToolImage> {
    projected_tool_images(items, None)
}

fn projected_tool_images(
    items: &[LogicalConversationItem],
    previous: Option<&MediaPrefix>,
) -> Vec<ToolImage> {
    // Newest first: each screenshot as its own group, and the latest media result's group.
    let mut groups = Vec::new();
    let mut screenshots = Vec::new();
    let mut media_seen = false;
    for item in items.iter().rev() {
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
        if tool_name == "media.frames" && (!media_seen || previous.is_some()) {
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
        } else if matches!(
            tool_name.as_str(),
            "browser.screenshot"
                | "chrome.screenshot"
                | "computer.screenshot"
                | "computer.act"
                | "computer.input"
                | "fs.read"
        ) && !*is_error
            && let Some(id) = serde_json::from_str::<serde_json::Value>(content)
                .ok()
                .as_ref()
                .and_then(shown_screenshot)
        {
            let shown = if tool_name == "chrome.screenshot" {
                "Chrome screenshot"
            } else if tool_name.starts_with("computer.") {
                "desktop screenshot"
            } else if tool_name == "fs.read" {
                "picture file"
            } else {
                "browser screenshot"
            };
            screenshots.push(groups.len());
            images.push((id, shown.to_owned()));
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
    // The older screenshots beyond what is carried leave; the media result's group stays.
    let left = if previous.is_none() {
        screenshots.split_off(carried(screenshots.len()))
    } else {
        Vec::new()
    };
    groups
        .into_iter()
        .enumerate()
        .filter(|(index, _)| left.binary_search(index).is_err())
        .rev()
        .flat_map(|(_, group)| group)
        .filter(|image| {
            previous.is_none_or(|prefix| prefix.admits(image.result_event, image.artifact_id))
        })
        .collect()
}

impl AgentApplication {
    /// The newest screenshots of the history, read from the conversation's store; none for a model
    /// that is not sent images.
    pub async fn screenshots(
        self: &Arc<Self>,
        run_id: AgentRunId,
        session: SessionId,
        items: &[LogicalConversationItem],
        current: &[EventId],
        takes_images: bool,
        previous: Option<&ContextProjectionSnapshot>,
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
        let previous = previous.map(MediaPrefix::new);
        let wanted = match &previous {
            Some(prefix) => projected_tool_images(items, Some(prefix)),
            None => tool_images(items),
        };
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
                    // conversation cannot read is left out too, and named as not sent.
                    let mut pictures = Vec::new();
                    let mut unread = Vec::new();
                    for message in messages {
                        let Some((_, attached)) =
                            aura_persistence_v2::agent::attachments::of_message(db, message)?
                        else {
                            continue;
                        };
                        for attachment in attached.iter().rev() {
                            if let AgentMessageAttachment::Image { name, artifact_id, .. } =
                                attachment
                            {
                                if !previous
                                    .as_ref()
                                    .is_none_or(|prefix| prefix.admits(message, *artifact_id))
                                    || (pictures.len() >= PICTURES
                                        && previous.as_ref().is_none_or(|prefix| {
                                            !prefix.attachments.contains(&(message, *artifact_id))
                                        }))
                                {
                                    continue;
                                }
                                match artifact::read(db, *artifact_id, &session.to_string()) {
                                    Ok(bytes) => {
                                        pictures.push((message, *artifact_id, name.clone(), bytes))
                                    }
                                    Err(_) => unread.push((message, name.clone())),
                                }
                            }
                        }
                    }
                    Ok((shots, pictures, unread))
                },
            )
            .await?;
        let (read, pictures, mut unread) = read;
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
        // In the order they were sent; bytes that are no picture are not sent either.
        for (message, id, name, bytes) in pictures.into_iter().rev() {
            let Some(media_type) =
                crate::agent_application::browser_screenshot::image_media_type(&bytes)
            else {
                unread.push((message, name));
                continue;
            };
            shots.push(Shot {
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
            });
        }
        Ok(Screenshots { shots, unread })
    }
}

impl Screenshots {
    /// Attaches the screenshots whose results the request still carries as they were, and names
    /// the pictures of the messages it carries that could not be read.
    pub fn attach(
        &self,
        input: &mut ContextPreparationInput,
        deferred: &std::collections::BTreeSet<ArtifactId>,
    ) -> Vec<String> {
        // The step's own messages, which the request adds after the history.
        let current = current_messages(&input.trigger);
        let carried = |message: &EventId| {
            current.contains(message)
                || input.recent_history_items.iter().any(|item| {
                    matches!(item, LogicalConversationItem::User { source_event_id, .. }
                        if source_event_id == message)
                })
        };
        let previous = input.previous_snapshot.as_ref().map(MediaPrefix::new);
        let retained: BTreeSet<_> =
            projected_tool_images(&input.recent_history_items, previous.as_ref())
                .into_iter()
                .map(|image| (image.result_event, image.artifact_id))
                .collect();
        let kept =
            self.shots
                .iter()
                .filter(|shot| !deferred.contains(&shot.attachment.snapshot.artifact_id))
                .filter(|shot| match &shot.place {
                    Place::Result => retained
                        .contains(&(shot.result_event, shot.attachment.snapshot.artifact_id)),
                    Place::Message => carried(&shot.result_event),
                })
                .collect::<Vec<_>>();
        let unread = self
            .unread
            .iter()
            .filter(|(message, _)| carried(message))
            .map(|(_, name)| name.clone())
            .collect();
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
        unread
    }

    /// The bytes of the request's attachments, in its order.
    pub fn resolved(
        &self,
        attachments: &[ModelAttachmentSnapshot],
    ) -> anyhow::Result<Vec<ResolvedModelAttachment>> {
        attachments
            .iter()
            .map(|snapshot| {
                self.shots
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

#[cfg(test)]
#[path = "screenshots_tests.rs"]
mod tests;
