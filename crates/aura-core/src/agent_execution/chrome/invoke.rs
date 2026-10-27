//! Carrying out an authorized Chrome call in the extension, and recording what came back.
//!
//! The extension's report is checked against the tool's own result type before the model sees it.
//! A refusal changed nothing; a failure or a silence after an action that can change the page is
//! recorded as unknown, and never run again on its own.
use super::super::*;
use super::resolve::timeout;
use aura_chrome_domain::commands::*;
use aura_chrome_domain::*;
use serde_json::{Value, json};

/// A screenshot's PNG bytes, width and height.
type Image = (Vec<u8>, u32, u32);

struct Answer {
    success: bool,
    side_effect: SideEffectStateV1,
    output: Option<Value>,
    error: Option<String>,
    image: Option<Image>,
}

fn failed(message: String, may_have_acted: bool) -> Answer {
    Answer {
        success: false,
        side_effect: if may_have_acted {
            SideEffectStateV1::Unknown
        } else {
            SideEffectStateV1::NoEffect
        },
        output: None,
        error: Some(message),
        image: None,
    }
}

/// Whether the command can change something in the user's browser.
fn acts(command: &ChromeCommand) -> bool {
    matches!(
        command,
        ChromeCommand::Open { .. }
            | ChromeCommand::Navigate { .. }
            | ChromeCommand::Click { .. }
            | ChromeCommand::Type { .. }
            | ChromeCommand::Press { .. }
            | ChromeCommand::Select { .. }
            | ChromeCommand::Close { .. }
    )
}

fn page(reference: &str, report: ChromePageReport) -> ChromePageResult {
    ChromePageResult {
        tab: reference.to_owned(),
        url: report.url,
        title: report.title,
        snapshot: report.snapshot,
        snapshot_truncated: report.snapshot_truncated,
        console: report.console,
        dialogs: report.dialogs,
        notice: report.notice,
    }
}

/// The report in the tool's own result shape, keeping the share's address current.
fn checked(
    chrome: &crate::chrome_control::ChromeControl,
    session: SessionId,
    command: &ChromeCommand,
    reference: &str,
    value: Value,
    shown: bool,
) -> Result<(Value, Option<Image>), String> {
    let invalid = |error: serde_json::Error| format!("Chrome 返回了无法识别的结果：{error}");
    let tab_of =
        |tab: u32, report: &ChromePageReport| chrome.tab_changed(tab, &report.url, &report.title);
    let output = match command {
        ChromeCommand::Open { .. } => {
            let report: ChromeOpenReport = serde_json::from_value(value).map_err(invalid)?;
            let reference = chrome.add_opened(session, &report.tab);
            serde_json::to_value(page(&reference, report.page))
        }
        ChromeCommand::Screenshot { tab_id, .. } => {
            let report: ChromeScreenshotReport = serde_json::from_value(value).map_err(invalid)?;
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&report.png_base64)
                .map_err(|_| "Chrome 返回的截图无法解码".to_owned())?;
            chrome.tab_changed(*tab_id, &report.url, &report.title);
            let note = if shown {
                "截图作为图片随这条结果发给你；用户也可以在这次调用里查看。"
            } else {
                "截图已随这次调用保存，用户可以查看。当前模型不接收图片：请用 chrome.snapshot 以文字读取页面。"
            };
            let output = json!({
                "tab": reference,
                "url": report.url,
                "title": report.title,
                "width": report.width,
                "height": report.height,
                "shown_to_model": shown,
                "note": note,
            });
            return Ok((output, Some((bytes, report.width, report.height))));
        }
        ChromeCommand::Console { .. } => {
            let report: ChromeConsoleReport = serde_json::from_value(value).map_err(invalid)?;
            serde_json::to_value(ChromeConsoleResult {
                tab: reference.to_owned(),
                url: report.url,
                entries: report.entries,
                truncated: report.truncated,
            })
        }
        ChromeCommand::Close { tab_id } => {
            let report: ChromeCloseReport = serde_json::from_value(value).map_err(invalid)?;
            if report.closed {
                chrome.remove_tab(*tab_id);
            }
            serde_json::to_value(ChromeCloseResult { closed: report.closed })
        }
        ChromeCommand::Assist { tab_id, .. } => {
            let report: ChromeAssistReport = serde_json::from_value(value).map_err(invalid)?;
            if let Some(after) = &report.page {
                tab_of(*tab_id, after);
            }
            serde_json::to_value(ChromeAssistResult {
                outcome: report.outcome,
                page: report.page.map(|report| page(reference, report)),
            })
        }
        ChromeCommand::Navigate { tab_id, .. }
        | ChromeCommand::Snapshot { tab_id, .. }
        | ChromeCommand::Click { tab_id, .. }
        | ChromeCommand::Type { tab_id, .. }
        | ChromeCommand::Press { tab_id, .. }
        | ChromeCommand::Scroll { tab_id, .. }
        | ChromeCommand::Hover { tab_id, .. }
        | ChromeCommand::Select { tab_id, .. }
        | ChromeCommand::Wait { tab_id, .. } => {
            let report: ChromePageReport = serde_json::from_value(value).map_err(invalid)?;
            tab_of(*tab_id, &report);
            serde_json::to_value(page(reference, report))
        }
        ChromeCommand::Inspect { .. } | ChromeCommand::Release { .. } => {
            return Err("这个命令没有工具结果".into());
        }
    };
    output.map(|output| (output, None)).map_err(invalid)
}

pub(in crate::agent_execution) async fn invoke(
    service: Arc<AgentExecution>,
    call: CallContext,
    request: DispatchRequest,
    start_lease: tokio::sync::OwnedRwLockReadGuard<()>,
) -> anyhow::Result<()> {
    let verifier = service.clone();
    let verified = call.clone();
    let operation = request.operation.operation_id;
    let model_step = call.batch.invocation_identity.model_step_id;
    let shown = service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "chrome.verify_start"),
            StoreAccess::Read,
            move || {
                verifier.verify_source(&verified)?;
                let journal = verifier.journal.lock()?;
                let db = journal.database.connection();
                aura_persistence_v2::agent::checkpoint::verify_start_operation(db, operation)?;
                Ok(aura_persistence_v2::model::images::step_takes_images(db, model_step)?)
            },
        )
        .await?;
    let content = &request.operation.content;
    let command: ChromeCommand = serde_json::from_value(content.action.clone())?;
    let reference = content
        .resources
        .iter()
        .find_map(|resource| match resource {
            ResourceDescriptor::ChromeTab { tab_ref } => Some(tab_ref.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let site = content.resources.iter().find_map(|resource| match resource {
        ResourceDescriptor::ChromeOrigin { origin, confirmed } => {
            Some((origin.clone(), *confirmed))
        }
        _ => None,
    });
    let stop = service
        .active_calls
        .session(call.snapshot.session_id)?
        .into_iter()
        .find(|entry| entry.call.validated.call.tool_call_id == call.validated.call.tool_call_id)
        .ok_or_else(|| anyhow::anyhow!("chrome.call_not_active"))?
        .cancel
        .subscribe();
    // The call has started; a later permission change does not undo it.
    drop(start_lease);
    let answer = carry_out(&service, &call, &command, &reference, site, shown, stop).await;
    let image_id = answer.image.as_ref().map(|_| ArtifactId::new());
    let mut output = answer.output;
    if let (Some(output), Some(id)) = (output.as_mut(), image_id) {
        output["image_artifact_id"] = json!(id);
    }
    let evidence = BrokerEvidence {
        action_kind: request.operation.content.action_kind.clone(),
        request_fingerprint: request.operation.fingerprint.0.clone(),
        side_effect: answer.side_effect,
        before: request.plan.prepared.preparation["domain"].clone(),
        after: Value::Null,
        domain_journal: json!({"schema":"agent-chrome-v1","output":output,"error":answer.error}),
    };
    let owner = service.clone();
    let image = answer.image;
    service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "chrome.result"),
            StoreAccess::Write,
            move || {
                Ok(owner.journal.lock()?.database.unit_of_work(|tx| {
                    let now = UtcTimestamp::now();
                    if let (Some(id), Some((bytes, _, _))) = (image_id, image.as_ref()) {
                        let owner = request.operation.operation_id.to_string();
                        aura_persistence_core::artifact::put(
                            tx,
                            id,
                            &owner,
                            "image/png",
                            bytes,
                            now,
                        )?;
                        // The conversation reads it back to send it to a model that takes
                        // images, and the run may cite it as one of its own results.
                        aura_persistence_core::artifact::grant_access(
                            tx,
                            id,
                            &owner,
                            &call.snapshot.session_id.to_string(),
                        )?;
                        aura_persistence_core::artifact::grant_access(
                            tx,
                            id,
                            &owner,
                            &call.batch.invocation_identity.agent_run_id.to_string(),
                        )?;
                    }
                    aura_persistence_v2::execution::broker_result::record(
                        tx,
                        &request,
                        &evidence,
                        answer.success,
                        now,
                    )
                })?)
            },
        )
        .await?;
    Ok(())
}

async fn carry_out(
    service: &AgentExecution,
    call: &CallContext,
    command: &ChromeCommand,
    reference: &str,
    site: Option<(String, bool)>,
    shown: bool,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> Answer {
    let Some(chrome) = service.chrome.get() else {
        return failed("chrome.not_connected: Chrome 没有连接 Aura。".into(), false);
    };
    let session = call.snapshot.session_id;
    let _hold = match chrome.hold(session) {
        Ok(hold) => hold,
        Err(message) => return failed(message, false),
    };
    let tab = match command {
        ChromeCommand::Open { .. } => None,
        ChromeCommand::Navigate { tab_id, .. }
        | ChromeCommand::Snapshot { tab_id, .. }
        | ChromeCommand::Screenshot { tab_id, .. }
        | ChromeCommand::Inspect { tab_id, .. }
        | ChromeCommand::Click { tab_id, .. }
        | ChromeCommand::Type { tab_id, .. }
        | ChromeCommand::Press { tab_id, .. }
        | ChromeCommand::Scroll { tab_id, .. }
        | ChromeCommand::Hover { tab_id, .. }
        | ChromeCommand::Select { tab_id, .. }
        | ChromeCommand::Wait { tab_id, .. }
        | ChromeCommand::Console { tab_id, .. }
        | ChromeCommand::Close { tab_id }
        | ChromeCommand::Assist { tab_id, .. } => Some(*tab_id),
        ChromeCommand::Release { .. } => None,
    };
    // The user approved this site for the share in Aura; the share keeps it.
    if let (Some(tab), Some((site, false))) = (tab, site.as_ref()) {
        chrome.confirm_origin(tab, site);
    }
    // The tab must still be on the site the call was authorized for; opening and navigating
    // replace the page.
    let expected = match command {
        ChromeCommand::Open { .. } | ChromeCommand::Navigate { .. } => None,
        _ => site.map(|(site, _)| site),
    };
    let assist = match command {
        ChromeCommand::Assist { reason, .. } => Some(reason.as_str()),
        _ => None,
    };
    if let (Some(reason), Some(tab)) = (assist, tab) {
        chrome.set_with_user(tab, Some(reason));
    }
    let Ok(value) = serde_json::to_value(command) else {
        return failed("chrome.failed: 命令无法发送".into(), false);
    };
    let may_act = acts(command);
    let sent = tokio::select! {
        sent = chrome.request(value, expected, timeout(command)) => Some(sent),
        _ = stop.wait_for(|stopped| *stopped) => None,
    };
    if let (Some(_), Some(tab)) = (assist, tab) {
        chrome.set_with_user(tab, None);
    }
    use crate::chrome_control::ChromeRequestError as E;
    let unsure = if may_act {
        "不确定这一步是否已经执行，请先用 chrome.snapshot 查看页面。"
    } else {
        ""
    };
    match sent {
        None => failed(format!("chrome.cancelled: 这一步已取消。{unsure}"), may_act),
        Some(Err(E::NotConnected)) => {
            failed("chrome.not_connected: Chrome 没有连接 Aura，这一步没有执行。".into(), false)
        }
        Some(Err(E::TimedOut)) => {
            failed(format!("chrome.timed_out: Chrome 没有及时回应。{unsure}"), may_act)
        }
        Some(Err(E::Disconnected)) => {
            failed(format!("chrome.disconnected: Chrome 的连接断开了。{unsure}"), may_act)
        }
        Some(Err(E::Failed { code, message })) => {
            let refused = code.starts_with("chrome.refused") || code == "chrome.stopped";
            failed(format!("{code}: {message}"), may_act && !refused)
        }
        Some(Ok(value)) => match checked(chrome, session, command, reference, value, shown) {
            Ok((output, image)) => Answer {
                success: true,
                side_effect: if may_act {
                    SideEffectStateV1::FullyApplied
                } else {
                    SideEffectStateV1::NoEffect
                },
                output: Some(output),
                error: None,
                image,
            },
            Err(message) => failed(format!("chrome.failed: {message}"), may_act),
        },
    }
}
