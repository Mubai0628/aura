//! An admitted read-only operation sends one verified handle to the bounded host bridge,
//! validates its output, and records only model-visible artifact references.
use super::super::*;
use super::{result, source};
use aura_agent_domain::*;
use serde_json::{Value, json};

pub(in crate::agent_execution) async fn invoke(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> anyhow::Result<()> {
    let verifier = Arc::clone(&service);
    let verified = call.clone();
    let operation = request.operation.operation_id;
    let prepared = service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "media.verify_start"),
            StoreAccess::Read,
            move || {
                verifier.verify_source(&verified)?;
                aura_persistence_v2::agent::checkpoint::verify_start_operation(
                    verifier.journal.lock()?.database.connection(),
                    operation,
                )?;
                let source = source::open(&verifier, &verified)?;
                let shown = aura_persistence_v2::model::images::step_takes_images(
                    verifier.journal.lock()?.database.connection(),
                    verified.batch.invocation_identity.model_step_id,
                )?;
                Ok((source, shown))
            },
        )
        .await;
    // A later permission change does not undo a read that started with this verified handle.
    drop(start_lease);
    let decoded = async {
        let (source, shown) = prepared?;
        let action = match request.operation.content.action_kind.as_str() {
            "media.probe" => MediaHostActionV1::Probe,
            "media.frames" => {
                let input = source::frames(&request.operation.content.action)?;
                MediaHostActionV1::Frames {
                    times: input.times,
                    count: input.count,
                    sheet: input.sheet,
                    max_side: input.max_side,
                }
            }
            other => anyhow::bail!("media.unknown_action: 这个版本的 Aura 不能执行 {other}，什么都没读。"),
        };
        let stop = service
            .active_calls
            .session(call.snapshot.session_id)?
            .into_iter()
            .find(|entry| {
                entry.call.validated.call.tool_call_id == call.validated.call.tool_call_id
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "media.call_not_active: 这次视频调用在发给解码服务之前已经结束（回合停止了或调用被取消），什么都没读。"
                )
            })?
            .cancel
            .subscribe();
        let track = match action {
            MediaHostActionV1::Probe => {
                Some(super::header::video_track(source.file.file(), source.size).await)
            }
            MediaHostActionV1::Frames { .. } => None,
        };
        let command = MediaHostCommandV1 {
            command_id: String::new(),
            runtime_instance_id: service.runtime,
            session_id: call.snapshot.session_id,
            file_size: source.size,
            media_type: source.media_type,
            timeout_ms: MEDIA_TIMEOUT_MS,
            action: action.clone(),
        };
        let response = service.media.send(command, source.file, stop).await?;
        match response.outcome {
            MediaHostOutcomeV1::Completed { result: raw } => {
                result::checked(&action, raw, source.size, shown, track)
            }
            MediaHostOutcomeV1::Refused { message } => {
                anyhow::bail!(not_completed("media.refused", &message, track.as_ref()))
            }
            MediaHostOutcomeV1::Failed { message } => {
                anyhow::bail!(not_completed("media.failed", &message, track.as_ref()))
            }
        }
    }
    .await;
    let (success, output, error, images) = match decoded {
        Ok(answer) => (true, Some(answer.output), None, answer.images),
        Err(error) => (false, None, Some(model_text(&error)), vec![]),
    };
    let evidence = BrokerEvidence {
        action_kind: request.operation.content.action_kind.clone(),
        request_fingerprint: request.operation.fingerprint.0.clone(),
        side_effect: SideEffectStateV1::NoEffect,
        before: request.plan.prepared.preparation["domain"].clone(),
        after: Value::Null,
        domain_journal: json!({"schema":"agent-media-v1","output":output,"error":error}),
    };
    let owner = Arc::clone(&service);
    service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "media.result"),
            StoreAccess::Write,
            move || {
                Ok(owner.journal.lock()?.database.unit_of_work(|tx| {
                    let at = UtcTimestamp::now();
                    let artifact_owner = request.operation.operation_id.to_string();
                    for image in images {
                        aura_persistence_core::artifact::put(
                            tx,
                            image.id,
                            &artifact_owner,
                            "image/jpeg",
                            &image.bytes,
                            at,
                        )?;
                        aura_persistence_core::artifact::grant_access(
                            tx,
                            image.id,
                            &artifact_owner,
                            &call.snapshot.session_id.to_string(),
                        )?;
                        aura_persistence_core::artifact::grant_access(
                            tx,
                            image.id,
                            &artifact_owner,
                            &call.batch.invocation_identity.agent_run_id.to_string(),
                        )?;
                    }
                    aura_persistence_v2::execution::broker_result::record(
                        tx, &request, &evidence, success, at,
                    )
                })?)
            },
        )
        .await?;
    Ok(())
}

/// What the decoder in Main said about a read it did not complete, as one bounded sentence, with
/// the codec the file's header names when it was read: a codec Chromium cannot decode explains it.
fn not_completed(
    code: &str,
    message: &str,
    track: Option<&aura_media_container::VideoTrack>,
) -> String {
    let said = super::super::browser::ended(&super::super::browser::outside_detail(message, 600));
    let header = track
        .and_then(|track| track.codec.as_ref())
        .map(|codec| format!("文件头写的编码是 {codec}。"))
        .unwrap_or_default();
    if code == "media.refused" {
        format!("{code}: {said}{header}什么都没改动。")
    } else {
        format!("{code}: {said}{header}什么都没改动；请确认这是能正常播放的视频文件后再试。")
    }
}

/// The failure as the model reads it: Aura's own `media.*` sentences as they are, anything else
/// as a read that did not complete, with a bounded detail.
fn model_text(error: &anyhow::Error) -> String {
    let text = error.to_string();
    if text.starts_with("media.") {
        return text;
    }
    format!(
        "media.failed: Aura 没能完成这次视频读取（{}），什么都没改动；可以再调用一次。",
        super::super::browser::outside_detail(&text, 200)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_decoder_said_is_one_sentence_that_says_nothing_changed() {
        let failed =
            not_completed("media.failed", "媒体解码失败，Chromium无法读取这个视频。\n第二行", None);
        assert_eq!(
            failed,
            "media.failed: 媒体解码失败，Chromium无法读取这个视频。什么都没改动；请确认这是能正常播放的视频文件后再试。"
        );
        let refused = not_completed("media.refused", "请求的取帧时间超出视频时长", None);
        assert_eq!(refused, "media.refused: 请求的取帧时间超出视频时长。什么都没改动。");
        let hevc = aura_media_container::VideoTrack {
            codec: Some("H.265 (hvc1)".into()),
            ..Default::default()
        };
        assert_eq!(
            not_completed("media.failed", "媒体解码失败，Chromium无法读取这个视频。", Some(&hevc)),
            "media.failed: 媒体解码失败，Chromium无法读取这个视频。文件头写的编码是 H.265 (hvc1)。什么都没改动；请确认这是能正常播放的视频文件后再试。"
        );
    }

    #[test]
    fn an_error_from_outside_the_media_tools_is_named_as_a_read_that_did_not_complete() {
        let own = anyhow::anyhow!("media.no_answer: 视频解码在限定时间内没有回应。");
        assert_eq!(model_text(&own), own.to_string());
        let raw = anyhow::Error::new(std::io::Error::other("pipe {closed}\r\nmore"));
        let text = model_text(&raw);
        assert!(
            text.starts_with("media.failed: Aura 没能完成这次视频读取（pipe (closed)）"),
            "{text}"
        );
        assert!(!text.contains(['{', '}', '\n', '\r']), "{text}");
    }
}
