//! Carrying out an authorized computer call through the desktop host, and recording what came
//! back.
//!
//! The host's report is turned into the tool's own result type before the model sees it, with the
//! refs the model uses from then on. A step that was not delivered changed nothing; a failure that
//! may have reached the program, a stop or a cancel after an action is recorded as unknown, and
//! never run again on its own.
use super::super::*;
use super::{Frozen, text};
use aura_desktop_domain::*;
use aura_desktop_service::{DesktopControl, KnownElement, KnownShot};
use serde_json::{Value, json};
use std::time::Duration;

/// A screenshot's PNG and the artifact it is kept as.
type Image = (Vec<u8>, ArtifactId);

struct Answer {
    success: bool,
    side_effect: SideEffectStateV1,
    output: Option<Value>,
    error: Option<String>,
    /// A step that changed nothing, as an ordinary refusal of its own kind.
    rejection: Option<Value>,
    image: Option<Image>,
}

/// `message` starts with its code (`computer.timeout: …`), as the model reads it.
fn failed(message: String, may_have_acted: bool) -> Answer {
    Answer {
        success: false,
        side_effect: if may_have_acted {
            SideEffectStateV1::Unknown
        } else {
            SideEffectStateV1::NoEffect
        },
        output: None,
        rejection: if may_have_acted { None } else { rejection(&message) },
        error: Some(message),
        image: None,
    }
}

/// What kind of refusal a step that changed nothing was.
fn rejection(message: &str) -> Option<Value> {
    use aura_domain_common::ErrorCategory as Kind;
    let (code, _) = message.split_once(':')?;
    let (kind, retryable) = match code {
        "computer.timeout" => (Kind::Transport, true),
        "computer.desktop_locked" | "computer.not_foreground" | "computer.off" => {
            (Kind::Unavailable, false)
        }
        "computer.cancelled" | "computer.stopped" | "computer.taken_over" => {
            (Kind::Cancelled, false)
        }
        "computer.busy" => (Kind::Unavailable, false),
        "computer.refused" => (Kind::Permission, false),
        "computer.stale" | "computer.moved" => (Kind::Conflict, false),
        _ => return None,
    };
    Some(json!({"code": code, "category": kind, "message": message, "retryable": retryable}))
}

fn code_name(code: HostErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "internal".into())
}

fn app_of(command: &HostCommand) -> Option<&AppEntry> {
    match command {
        HostCommand::Launch { app, .. }
        | HostCommand::Input { app, .. }
        | HostCommand::Elements { app, .. }
        | HostCommand::Act { app, .. }
        | HostCommand::Window { app, .. } => Some(app),
        HostCommand::Screenshot { show, .. } => show.first(),
        HostCommand::Catalog { .. }
        | HostCommand::Windows { .. }
        | HostCommand::Displays
        | HostCommand::Icons { .. }
        | HostCommand::Inspect { .. } => None,
        HostCommand::Guarded { .. } | HostCommand::Release { .. } | HostCommand::Keep { .. } => {
            None
        }
    }
}

/// How long Core waits for the host, on top of the link's own margin.
pub(super) fn limit(command: &HostCommand) -> Duration {
    Duration::from_millis(match command {
        HostCommand::Launch { timeout_ms, .. } | HostCommand::Elements { timeout_ms, .. } => {
            u64::from(*timeout_ms) + 2_000
        }
        HostCommand::Act { timeout_ms, .. } => u64::from(*timeout_ms) + 37_000,
        HostCommand::Input { steps, .. } => {
            let waits: u64 = steps
                .iter()
                .map(|step| match step {
                    InputStep::Wait { ms } => u64::from(*ms),
                    _ => 0,
                })
                .sum();
            65_000 + waits
        }
        _ => 20_000,
    })
}

/// A screenshot's windows of the program in the screenshot's own pixels.
pub(super) fn windows_on_image(
    map: &ScreenMap,
    windows: &[WindowInfo],
) -> Vec<ComputerShotWindowV1> {
    let (across, down) = map.scale();
    windows
        .iter()
        .map(|window| ComputerShotWindowV1 {
            window: window.window.clone(),
            title: window.title.clone(),
            x: ((f64::from(window.rect.x) - f64::from(map.left)) / across).floor() as i64,
            y: ((f64::from(window.rect.y) - f64::from(map.top)) / down).floor() as i64,
            width: (f64::from(window.rect.width) / across).round() as i64,
            height: (f64::from(window.rect.height) / down).round() as i64,
            foreground: window.foreground,
        })
        .collect()
}

/// The screenshot as the tool's result, kept for its ref; the PNG goes into an artifact.
fn screenshot(
    desktop: &DesktopControl,
    session: SessionId,
    app: &AppEntry,
    result: HostResult,
    shown: bool,
    marks: usize,
) -> Result<(ComputerScreenshotResult, Vec<u8>), String> {
    let HostResult::Screenshot { png, map, windows, .. } = result else {
        return Err("截图时收到了别的结果".into());
    };
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png.as_bytes())
        .map_err(|_| "截图无法解码".to_owned())?;
    let on_image = windows_on_image(&map, &windows);
    let reference =
        desktop.remember_shot(session, KnownShot { app_id: app.id.clone(), map, windows });
    let mut note = if shown {
        "截图作为图片随这条结果发给你；用户也可以在这次调用里查看。别的程序的窗口已涂掉。"
    } else {
        "截图已随这次调用保存，用户可以查看。当前模型不接收图片：请用 computer.snapshot 以文字读取窗口。"
    }
    .to_owned();
    if marks > 0 {
        note.push_str("图上编号框的号 n 就是元素 en，可作为 computer.act、computer.input 的 ref。");
    }
    Ok((
        ComputerScreenshotResult {
            app: app.id.clone(),
            screenshot: reference,
            width: map.width,
            height: map.height,
            windows: on_image,
            image_artifact_id: ArtifactId::new(),
            shown_to_model: shown,
            marks: (marks > 0).then(|| u32::try_from(marks).unwrap_or(u32::MAX)),
            note,
        },
        bytes,
    ))
}

const STILL_OPEN: &str = "窗口还开着：程序可能在问要不要保存。先用 computer.snapshot 读取它的窗口，再按用户说的做；用户没说时问用户，不要自己选不保存。";

/// The host's report in the tool's own result shape.
fn output(
    desktop: &DesktopControl,
    session: SessionId,
    frozen: &Frozen,
    app: &AppEntry,
    result: HostResult,
    shown: bool,
) -> Result<(Value, Option<Image>), String> {
    let value = match (&frozen.command, result) {
        (HostCommand::Launch { .. }, HostResult::Launched { windows, already_running }) => {
            serde_json::to_value(ComputerOpenResult {
                app: app.id.clone(),
                windows,
                already_running,
            })
        }
        (HostCommand::Elements { .. }, HostResult::Elements { window, elements, complete }) => {
            let known = elements
                .iter()
                .map(|element| KnownElement {
                    window: window.window.clone(),
                    key: element.key.clone(),
                    rect: element.rect,
                    acts: !element.abilities.is_empty(),
                })
                .collect();
            let refs = desktop.remember_elements(session, &app.id, known);
            serde_json::to_value(ComputerSnapshotResult {
                app: app.id.clone(),
                listed: u32::try_from(elements.len()).unwrap_or(u32::MAX),
                elements: text::elements(&elements, &refs),
                window: window.window,
                title: window.title,
                complete,
            })
        }
        (HostCommand::Screenshot { marks, .. }, result @ HostResult::Screenshot { .. }) => {
            let (result, bytes) = screenshot(desktop, session, app, result, shown, marks.len())?;
            let id = result.image_artifact_id;
            let value = serde_json::to_value(result).map_err(|error| error.to_string())?;
            return Ok((value, Some((bytes, id))));
        }
        (HostCommand::Act { window, .. }, HostResult::Acted { after }) => {
            for (reference, element) in frozen.refs.iter().zip(&after) {
                if let Some(element) = element {
                    let known = KnownElement {
                        window: window.clone(),
                        key: element.key.clone(),
                        rect: element.rect,
                        acts: !element.abilities.is_empty(),
                    };
                    desktop.replace_element(session, &app.id, reference, known);
                }
            }
            serde_json::to_value(ComputerActResult {
                app: app.id.clone(),
                window: window.clone(),
                steps_done: u32::try_from(after.len()).unwrap_or(u32::MAX),
                after: text::after(&frozen.refs, &after),
                screenshot: None,
                screenshot_error: None,
            })
        }
        (HostCommand::Input { .. }, HostResult::Input { steps_done }) => {
            serde_json::to_value(ComputerInputResult {
                app: app.id.clone(),
                steps_done,
                screenshot: None,
                screenshot_error: None,
            })
        }
        (HostCommand::Window { action, .. }, HostResult::Window { window, changed, windows }) => {
            let open =
                *action == WindowAction::Close && windows.iter().any(|one| one.window == window);
            serde_json::to_value(ComputerWindowResult {
                app: app.id.clone(),
                window,
                action: *action,
                changed,
                windows,
                note: open.then(|| STILL_OPEN.to_owned()),
            })
        }
        (_, other) => return Err(format!("电脑操控返回了不对应的结果：{other:?}")),
    };
    value.map(|value| (value, None)).map_err(|error| error.to_string())
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
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "computer.verify_start"),
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
    let frozen: Frozen = serde_json::from_value(request.operation.content.action.clone())?;
    let stop = service
        .active_calls
        .session(call.snapshot.session_id)?
        .into_iter()
        .find(|entry| entry.call.validated.call.tool_call_id == call.validated.call.tool_call_id)
        .ok_or_else(|| anyhow::anyhow!("computer.call_not_active"))?
        .cancel
        .subscribe();
    // The call has started; a later permission change does not undo it.
    drop(start_lease);
    let answer = carry_out(&service, &call, &frozen, shown, stop).await;
    let evidence = BrokerEvidence {
        action_kind: request.operation.content.action_kind.clone(),
        request_fingerprint: request.operation.fingerprint.0.clone(),
        side_effect: answer.side_effect,
        before: request.plan.prepared.preparation["domain"].clone(),
        after: Value::Null,
        domain_journal: json!({"schema":"agent-computer-v1","output":answer.output,"error":answer.error,"operation_rejection":answer.rejection}),
    };
    let owner = service.clone();
    let image = answer.image;
    service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "computer.result"),
            StoreAccess::Write,
            move || {
                Ok(owner.journal.lock()?.database.unit_of_work(|tx| {
                    let now = UtcTimestamp::now();
                    if let Some((bytes, id)) = image.as_ref() {
                        let owner = request.operation.operation_id.to_string();
                        aura_persistence_core::artifact::put(
                            tx,
                            *id,
                            &owner,
                            "image/png",
                            bytes,
                            now,
                        )?;
                        // The conversation reads it back to send it to a model that takes
                        // images, and the run may cite it as one of its own results.
                        aura_persistence_core::artifact::grant_access(
                            tx,
                            *id,
                            &owner,
                            &call.snapshot.session_id.to_string(),
                        )?;
                        aura_persistence_core::artifact::grant_access(
                            tx,
                            *id,
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
    service: &Arc<AgentExecution>,
    call: &CallContext,
    frozen: &Frozen,
    shown: bool,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) -> Answer {
    let Some(desktop) = service.desktop.get() else {
        return failed(super::resolve::OFF.into(), false);
    };
    let desktop = match desktop.select(frozen.mode) {
        Ok(desktop) => desktop,
        Err(error) => return failed(format!("computer.unavailable: {}", error.message), false),
    };
    let Some(app) = app_of(&frozen.command).cloned() else {
        return failed("computer.unsupported_tool: 这个命令没有工具结果".into(), false);
    };
    let session = call.snapshot.session_id;
    let turn = call.operation.content.identity.turn_task_id;
    let may_act = frozen.command.acts_on().is_some();
    if turn.is_some_and(|turn| desktop.turn_stopped(session, turn, &app.id)) && may_act {
        return failed("computer.stopped: 用户已停止，这一回合没有执行".into(), false);
    }
    let mut controlling =
        desktop.begin_turn(session, &app, frozen.command.takes_foreground(), turn);
    let unsure = if may_act {
        "不确定这一步做到了哪里，请先用 computer.snapshot 或 computer.screenshot 看看程序现在的样子。"
    } else {
        ""
    };
    let caller =
        aura_desktop_service::Caller { call: call.validated.call.tool_call_id, session, turn };
    let reply = tokio::select! {
        reply = desktop.run_for(frozen.command.clone(), limit(&frozen.command), caller) => reply,
        _ = cancel.wait_for(|cancelled| *cancelled) => {
            return failed(format!("computer.cancelled: 这一步已取消。{unsure}"), may_act);
        }
        _ = controlling.stopped.changed() => {
            return failed(
                format!("computer.stopped: 用户停止了 {} 的电脑操控；已确认前 {} 步完成，其余步骤不确定；这一回合不要再操作它。{unsure}", app.name, desktop.steps_done_for(call.validated.call.tool_call_id, &app.id)),
                may_act,
            );
        }
    };
    let result = match reply {
        Ok(result) => result,
        Err(error) => {
            let done =
                error.steps_done.map(|steps| format!("（前 {steps} 步已做）")).unwrap_or_default();
            let uncertain = error.delivery == Delivery::Uncertain;
            let tail = if uncertain && may_act { unsure } else { "" };
            return failed(
                format!("computer.{}: {}{done}{tail}", code_name(error.code), error.message),
                may_act && uncertain,
            );
        }
    };
    let (mut value, mut image) = match output(&desktop, session, frozen, &app, result, shown) {
        Ok(output) => output,
        Err(message) => return failed(format!("computer.failed: {message}{unsure}"), may_act),
    };
    if let Some(after) = &frozen.after {
        // The step itself is done; a screenshot that fails afterwards only says so.
        match desktop.run(after.clone(), limit(after)).await {
            Ok(result) => match screenshot(&desktop, session, &app, result, shown, 0) {
                Ok((shot, bytes)) => {
                    image = Some((bytes, shot.image_artifact_id));
                    value["screenshot"] = serde_json::to_value(shot).unwrap_or(Value::Null);
                }
                Err(message) => value["screenshot_error"] = json!(message),
            },
            Err(error) => value["screenshot_error"] = json!(error.message),
        }
    }
    // Opening a program that runs already, or a window that already was as asked.
    let unchanged = match frozen.command {
        HostCommand::Launch { .. } => value["already_running"] == json!(true),
        HostCommand::Window { .. } => value["changed"] == json!(false),
        _ => false,
    };
    Answer {
        success: true,
        side_effect: if may_act && !unchanged {
            SideEffectStateV1::FullyApplied
        } else {
            SideEffectStateV1::NoEffect
        },
        output: Some(value),
        error: None,
        rejection: None,
        image,
    }
}
