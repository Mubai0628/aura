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
                let input: MediaFramesInput =
                    serde_json::from_value(request.operation.content.action.clone())?;
                input.validate().map_err(anyhow::Error::msg)?;
                MediaHostActionV1::Frames {
                    times: input.times,
                    count: input.count,
                    sheet: input.sheet,
                    max_side: input.max_side,
                }
            }
            _ => anyhow::bail!("media.unknown_action"),
        };
        let stop = service
            .active_calls
            .session(call.snapshot.session_id)?
            .into_iter()
            .find(|entry| {
                entry.call.validated.call.tool_call_id == call.validated.call.tool_call_id
            })
            .ok_or_else(|| anyhow::anyhow!("media.call_not_active"))?
            .cancel
            .subscribe();
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
                result::checked(&action, raw, source.size, shown)
            }
            MediaHostOutcomeV1::Refused { message } => {
                anyhow::bail!("media.refused: {}", message.chars().take(1024).collect::<String>())
            }
            MediaHostOutcomeV1::Failed { message } => {
                anyhow::bail!("media.failed: {}", message.chars().take(1024).collect::<String>())
            }
        }
    }
    .await;
    let (success, output, error, images) = match decoded {
        Ok(answer) => (true, Some(answer.output), None, answer.images),
        Err(error) => (false, None, Some(error.to_string()), vec![]),
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
