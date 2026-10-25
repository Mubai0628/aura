//! Real Agent Store reads and final attachment selection, without a provider or media bridge.
#![allow(clippy::unwrap_used)]
use super::*;
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::ToolCancellationReasonV1;
use serde_json::json;

const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0, 1];

async fn fixture() -> (crate::r03_tests::Fixture, crate::RuntimeState, Arc<AgentApplication>) {
    let root = crate::r03_tests::Fixture::new();
    let runtime = root.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("fixture ready")
    };
    let app = services.agent_required();
    (root, runtime, app)
}

fn images(app: &AgentApplication, session: SessionId, count: usize) -> Vec<ArtifactId> {
    let mut journal = app.execution.journal.lock().expect("fixture journal");
    journal
        .database
        .unit_of_work(|tx| {
            (0..count)
                .map(|_| {
                    let id = ArtifactId::new();
                    artifact::put(
                        tx,
                        id,
                        &session.to_string(),
                        "image/jpeg",
                        JPEG,
                        UtcTimestamp::now(),
                    )?;
                    Ok(id)
                })
                .collect::<aura_persistence_core::StoreResult<Vec<_>>>()
        })
        .expect("persist real image artifacts")
}

fn result(tool: &str, value: serde_json::Value) -> LogicalConversationItem {
    let outcome = ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
    };
    let content = model_view(outcome).to_string();
    LogicalConversationItem::ToolResult {
        source_event_id: EventId::new(),
        tool_call_id: ToolCallId::new(),
        tool_name: tool.to_owned(),
        content_hash: ContentHash::sha256(content.as_bytes()),
        content,
        is_error: false,
    }
}

/// The actual model_view shape published by ModelVisibleToolResultEnvelopeV1::model_text:
/// image references live only in the typed outcome, while artifacts list the stored result.
fn model_view(outcome: ModelVisibleToolResultOutcomeV1) -> serde_json::Value {
    let byte_length = serde_json::to_vec(&outcome).unwrap().len();
    json!({
        "artifacts": [{"artifact_id":ArtifactId::new(),"byte_length":byte_length,
            "media_type":"application/vnd.aura.model-tool-outcome+json","slot":"result"}],
        "outcome": outcome,
        "side_effect_state": SideEffectStateV1::NoEffect,
    })
}

fn frames(ids: &[ArtifactId], shown: bool, sheet: Option<ArtifactId>) -> LogicalConversationItem {
    result(
        "media.frames",
        json!({
            "frames": ids.iter().enumerate().map(|(i, id)| json!({"requested_time": i, "actual_time": i, "image_artifact_id": id})).collect::<Vec<_>>(),
            "sheet_artifact_id": sheet,
            "shown_to_model": shown,
            "note": "",
        }),
    )
}

fn input(items: Vec<LogicalConversationItem>) -> ContextPreparationInput {
    let mut input = aura_context_fixtures::p06_performance_preparation_input(128_000, 20)
        .expect("context fixture");
    input.recent_history_items = items;
    input
}

#[tokio::test]
async fn l13_3_media_images_carry_latest_twelve_frames_plus_three_screenshots_and_twenty_user_pictures()
 {
    let (_root, _runtime, app) = fixture().await;
    let session = SessionId::new();
    let browser = images(&app, session, 4);
    let old = images(&app, session, 2);
    let latest = images(&app, session, 12);
    let mut history = browser
        .iter()
        .map(|id| {
            result("browser.screenshot", json!({"image_artifact_id": id, "shown_to_model": true}))
        })
        .collect::<Vec<_>>();
    history.push(frames(&old, true, None));
    history.push(frames(&latest, true, None));
    let event = EventId::new();
    let user_ids = {
        let mut journal = app.execution.journal.lock().expect("fixture journal");
        journal
            .database
            .unit_of_work(|tx| -> aura_persistence_core::StoreResult<Vec<ArtifactId>> {
                let prepared = (0..21)
                    .map(|i| aura_persistence_v2::agent::attachments::PreparedAttachment::Image {
                        name: format!("user-{i}.jpg"),
                        media_type: "image/jpeg".to_owned(),
                        bytes: JPEG.to_vec(),
                    })
                    .collect::<Vec<_>>();
                let kept = aura_persistence_v2::agent::attachments::keep(
                    tx,
                    session,
                    &prepared,
                    UtcTimestamp::now(),
                )?;
                aura_persistence_v2::agent::attachments::record(
                    tx,
                    event,
                    "pictures",
                    &kept,
                    UtcTimestamp::now(),
                )?;
                Ok(kept
                    .into_iter()
                    .map(|attachment| match attachment {
                        AgentMessageAttachment::Image { artifact_id, .. } => artifact_id,
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>())
            })
            .expect("persist user pictures")
    };
    history.push(LogicalConversationItem::User {
        source_event_id: event,
        content: "pictures".into(),
        content_hash: ContentHash::sha256(b"pictures"),
    });
    let shots = app
        .screenshots(AgentRunId::new(), session, &history, &[], true)
        .await
        .expect("load context images");
    let mut request = input(history);
    shots.attach(&mut request);
    let ids = request.attachments.iter().map(|a| a.artifact_id).collect::<Vec<_>>();
    assert_eq!(ids.len(), 35, "12 latest video frames, 3 screenshots, 20 user pictures");
    assert_eq!(&ids[..3], &browser[1..]);
    assert_eq!(&ids[3..15], latest.as_slice(), "frames stay in requested order");
    assert_eq!(&ids[15..], &user_ids[1..]);
    assert!(!ids.iter().any(|id| old.contains(id)), "previous media result is not carried");
    assert_eq!(shots.resolved(&request.attachments).expect("resolve bytes").len(), 35);
}

#[tokio::test]
async fn l13_3_media_images_a_sheet_is_one_image_and_a_non_image_model_receives_none() {
    let (_root, _runtime, app) = fixture().await;
    let session = SessionId::new();
    let sheet = images(&app, session, 1)[0];
    let history = vec![result(
        "media.frames",
        json!({"frames":[{"requested_time":0,"actual_time":0,"image_artifact_id":null},{"requested_time":1,"actual_time":1,"image_artifact_id":null}],"sheet_artifact_id":sheet,"shown_to_model":true,"note":""}),
    )];
    let shots =
        app.screenshots(AgentRunId::new(), session, &history, &[], true).await.expect("sheet");
    let mut request = input(history.clone());
    shots.attach(&mut request);
    assert_eq!(request.attachments.iter().map(|a| a.artifact_id).collect::<Vec<_>>(), vec![sheet]);
    let none = app
        .screenshots(AgentRunId::new(), session, &history, &[], false)
        .await
        .expect("no image model");
    none.attach(&mut request);
    assert!(request.attachments.is_empty());
}

#[tokio::test]
async fn l13_3_media_images_hidden_latest_results_replace_old_frames_and_other_sessions_are_denied()
{
    let (_root, _runtime, app) = fixture().await;
    let session = SessionId::new();
    let own = images(&app, session, 2);
    let foreign = images(&app, SessionId::new(), 1);
    let denied = app
        .screenshots(AgentRunId::new(), session, &[frames(&foreign, true, None)], &[], true)
        .await
        .expect("foreign safely omitted");
    assert!(denied.0.is_empty(), "cannot read another session's artifact");
    let mut failed = frames(&own[1..], true, None);
    if let LogicalConversationItem::ToolResult { content, content_hash, is_error, .. } = &mut failed
    {
        *content = model_view(ModelVisibleToolResultOutcomeV1::Cancelled {
            reason: ToolCancellationReasonV1::UserRequested,
        })
        .to_string();
        *content_hash = ContentHash::sha256(content.as_bytes());
        *is_error = true;
    }
    for latest in [
        frames(&own[1..], false, None),
        frames(&[], true, None),
        failed,
        result("media.frames", json!({"unavailable":true})),
    ] {
        let history = vec![frames(&own[..1], true, None), latest];
        let shots = app
            .screenshots(AgentRunId::new(), session, &history, &[], true)
            .await
            .expect("latest result");
        assert!(shots.0.is_empty(), "a newer unavailable result must not revive the prior frame");
    }
}

#[tokio::test]
async fn l13_3_media_images_only_still_shown_retained_results_attach_and_frame_count_is_bounded() {
    let (_root, _runtime, app) = fixture().await;
    let session = SessionId::new();
    let ids = images(&app, session, 13);
    let history = vec![frames(&ids, true, None)];
    let shots = app
        .screenshots(AgentRunId::new(), session, &history, &[], true)
        .await
        .expect("bounded frames");
    let mut request = input(history.clone());
    shots.attach(&mut request);
    assert_eq!(request.attachments.len(), 12);
    request.recent_history_items.clear();
    shots.attach(&mut request);
    assert!(request.attachments.is_empty(), "compacted/cleared result no longer sends images");
    request.recent_history_items = history;
    if let LogicalConversationItem::ToolResult { content, content_hash, .. } =
        &mut request.recent_history_items[0]
    {
        *content = content.replace("\"shown_to_model\":true", "\"shown_to_model\":false");
        *content_hash = ContentHash::sha256(content.as_bytes());
    }
    shots.attach(&mut request);
    assert!(request.attachments.is_empty(), "artifact text alone does not mean it remains shown");
    if let LogicalConversationItem::ToolResult { content, content_hash, .. } =
        &mut request.recent_history_items[0]
    {
        *content = model_view(ModelVisibleToolResultOutcomeV1::Cancelled {
            reason: ToolCancellationReasonV1::UserRequested,
        })
        .to_string();
        *content_hash = ContentHash::sha256(content.as_bytes());
    }
    shots.attach(&mut request);
    assert!(request.attachments.is_empty(), "a retained non-success outcome cannot attach images");
}

#[test]
fn l13_3_media_images_accept_only_the_named_tools_typed_success_payload() {
    let id = ArtifactId::new();
    let good = frames(&[id], true, None);
    assert_eq!(tool_images(std::slice::from_ref(&good))[0].artifact_id, id);
    let LogicalConversationItem::ToolResult { content, .. } = &good else { unreachable!() };
    let view: serde_json::Value = serde_json::from_str(content).unwrap();
    let bare = view["outcome"]["payload"]["value"].clone();
    for bad in [
        bare.clone(),
        json!({"artifacts":[bare.clone()]}),
        json!({"outcome":{"type":"succeeded","payload":{"type":"utf8_text","text":bare.to_string()}}}),
        json!({"outcome":{"type":"cancelled","reason":"user_requested"},"artifacts":[bare]}),
        json!({"nested":view}),
    ] {
        assert!(media_result(&bad.to_string()).is_none(), "non-payload image reference: {bad}");
    }
    // Unified tools.call is canonically renamed by model_context::executed_name before context
    // selection (the real wire test covers that path). Payload shape alone cannot infer a tool.
    let mut unnamed = good;
    if let LogicalConversationItem::ToolResult { tool_name, .. } = &mut unnamed {
        *tool_name = "tools.call".into();
    }
    assert!(tool_images(&[unnamed]).is_empty());
}
