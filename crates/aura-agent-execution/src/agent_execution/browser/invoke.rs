//! Carrying out an authorized browser call on the host, and recording what came back.
//!
//! The host's result is checked against the tool's own result type before the model sees it. A
//! refusal (page moved, user in control, stale ref, sensitive field) changed nothing; a failure or
//! a silence after an input action may have changed the page, and is recorded as unknown.
use super::super::*;
use super::bridge::BridgeError;
use super::resolve::timeout_ms;
use aura_browser_domain::*;
use serde_json::{Value, json};

/// After a step that may have changed the page and did not report back.
const UNSURE: &str = "不确定这一步是否已经执行，请先用 browser.snapshot 查看页面。";
/// After a step that did not change the page.
const UNCHANGED: &str = "页面没有被改动；可以先用 browser.snapshot 查看页面，再决定是否重试。";

/// What the browser said the call did.
struct Answer {
    success: bool,
    side_effect: SideEffectStateV1,
    output: Option<Value>,
    error: Option<String>,
    image: Option<BrowserImageV1>,
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

/// The host's result in the tool's own shape, or why it is not. `shown`: the step's model is sent
/// images, so a screenshot goes to it with this result.
fn checked(
    action: &BrowserHostActionV1,
    result: Value,
    image: Option<&BrowserImageV1>,
    shown: bool,
) -> Result<Value, String> {
    let invalid = |error: serde_json::Error| {
        format!(
            "浏览器返回的结果 Aura 读不懂（{}）",
            super::outside_detail(&error.to_string(), 200)
        )
    };
    match action {
        BrowserHostActionV1::Screenshot { .. } => {
            #[derive(serde::Deserialize)]
            struct Shot {
                url: String,
                title: String,
            }
            let shot: Shot = serde_json::from_value(result).map_err(invalid)?;
            let image = image.ok_or("浏览器没有返回截图")?;
            let note = if shown {
                "截图作为图片随这条结果发给你（最近几张截图会一直附带）；用户也可以在这次调用里查看。"
            } else {
                "截图已随这次调用保存，用户可以查看。当前模型不接收图片：请用 browser.snapshot 以文字读取页面。"
            };
            Ok(json!({
                "url": shot.url,
                "title": shot.title,
                "width": image.width,
                "height": image.height,
                "shown_to_model": shown,
                "note": note,
            }))
        }
        BrowserHostActionV1::Console { .. } => serde_json::to_value(
            serde_json::from_value::<BrowserConsoleResult>(result).map_err(invalid)?,
        )
        .map_err(invalid),
        BrowserHostActionV1::Close => serde_json::to_value(
            serde_json::from_value::<BrowserCloseResult>(result).map_err(invalid)?,
        )
        .map_err(invalid),
        BrowserHostActionV1::RequestUser { .. } => serde_json::to_value(
            serde_json::from_value::<BrowserAssistResult>(result).map_err(invalid)?,
        )
        .map_err(invalid),
        BrowserHostActionV1::Evaluate { .. } => serde_json::to_value(
            serde_json::from_value::<BrowserEvaluateResult>(result).map_err(invalid)?,
        )
        .map_err(invalid),
        BrowserHostActionV1::Network { .. } => serde_json::to_value(
            serde_json::from_value::<BrowserNetworkResult>(result).map_err(invalid)?,
        )
        .map_err(invalid),
        BrowserHostActionV1::Inspect { .. } => serde_json::to_value(
            serde_json::from_value::<BrowserInspectResult>(result).map_err(invalid)?,
        )
        .map_err(invalid),
        BrowserHostActionV1::Performance { .. } => serde_json::to_value(
            serde_json::from_value::<BrowserPerformanceResult>(result).map_err(invalid)?,
        )
        .map_err(invalid),
        _ => serde_json::to_value(
            serde_json::from_value::<BrowserPageResult>(result).map_err(invalid)?,
        )
        .map_err(invalid),
    }
}

fn answer_of(
    action: &BrowserHostActionV1,
    sent: Result<BrowserHostResultV1, BridgeError>,
    shown: bool,
) -> Answer {
    // A navigation or an input that went unanswered may have happened.
    let acts = action.is_input()
        || matches!(
            action,
            BrowserHostActionV1::Open { .. } | BrowserHostActionV1::Navigate { .. }
        );
    let result = match sent {
        Ok(result) => result,
        Err(error @ BridgeError::NotConnected) => return failed(error.to_string(), false),
        Err(error) => {
            let tail = if acts { UNSURE } else { UNCHANGED };
            return failed(format!("{error}{tail}"), acts);
        }
    };
    match result.outcome {
        BrowserHostOutcomeV1::Refused { message } => {
            failed(format!("browser.refused: {message}"), false)
        }
        // The host's own sentence around the page's exception; an older host sent the bare text.
        BrowserHostOutcomeV1::Failed { message, may_have_acted } => {
            let unsure = may_have_acted && acts;
            let said = super::ended(&super::outside_detail(&message, 600));
            failed(
                format!("browser.failed: {said}{}", if unsure { UNSURE } else { UNCHANGED }),
                unsure,
            )
        }
        BrowserHostOutcomeV1::Completed { result, image } => {
            match checked(action, result, image.as_ref(), shown) {
                Ok(output) => Answer {
                    success: true,
                    side_effect: if action.is_input() {
                        SideEffectStateV1::FullyApplied
                    } else {
                        SideEffectStateV1::NoEffect
                    },
                    output: Some(output),
                    error: None,
                    image,
                },
                // The host finished the step; only its report could not be used.
                Err(message) if acts => failed(
                    format!(
                        "browser.failed: 这一步在浏览器里已经完成，但{message}；请先用 browser.snapshot 查看页面，不要直接重做。"
                    ),
                    true,
                ),
                Err(message) => failed(format!("browser.failed: {message}。{UNCHANGED}"), false),
            }
        }
    }
}

impl AgentExecution {
    /// The command for the host: the action Core resolved, the page's authorized site, and how
    /// the page may navigate while it is carried out.
    /// The command, and whether the step's model is sent images.
    fn browser_command(
        &self,
        call: &CallContext,
        request: &DispatchRequest,
    ) -> anyhow::Result<(BrowserHostCommandV1, bool)> {
        let content = &request.operation.content;
        let action: BrowserHostActionV1 = serde_json::from_value(content.action.clone())?;
        let session = call.snapshot.session_id;
        let target = content.resources.iter().find_map(|resource| match resource {
            ResourceDescriptor::NetworkOrigin { host, .. } => Some(host.clone()),
            _ => None,
        });
        // The page the call was resolved and approved on: a page the user handed over, or the
        // session's model page.
        let page = content
            .resources
            .iter()
            .find_map(|resource| match resource {
                ResourceDescriptor::BrowserPage { page_ref } => {
                    page_ref.rsplit_once('/').map(|(_, page)| page.to_owned())
                }
                _ => None,
            })
            .filter(|page| page != super::resolve::MODEL_PAGE);
        let (mut approved_hosts, images) = {
            let journal = self.journal.lock()?;
            let db = journal.database.connection();
            (
                aura_persistence_v2::network_consents::browser_hosts(db, session)?,
                aura_persistence_v2::model::images::step_takes_images(
                    db,
                    call.batch.invocation_identity.model_step_id,
                )?,
            )
        };
        approved_hosts.extend(target.clone());
        // A page that shows a file of a sub-agent's worktree may load only that copy's files.
        let shown = match &action {
            BrowserHostActionV1::Open { url } => Some(url.clone()),
            _ => self
                .browser
                .page_url(session, page.as_deref().unwrap_or(super::resolve::MODEL_PAGE)),
        };
        let copy = match shown {
            Some(url) => self.copy_holding(session, &url)?,
            None => None,
        };
        let root = match copy {
            Some(copy) => copy,
            None => {
                self.workspace.resolver(call.snapshot.workspace_id)?.canonical_root().to_path_buf()
            }
        };
        let file_root = url::Url::from_directory_path(&root).ok().map(|url| url.to_string());
        Ok((
            BrowserHostCommandV1 {
                command_id: String::new(),
                runtime_instance_id: self.runtime,
                session_id: session,
                page,
                // Opening a page replaces it; any other call is held to the site it was approved on.
                expected_host: if matches!(action, BrowserHostActionV1::Open { .. }) {
                    None
                } else {
                    aura_security_contract::browser_site(&content.resources)
                        .map(|(host, _)| host.to_owned())
                },
                access: BrowserPageAccessV1 {
                    read_only: call.snapshot.mode == PermissionMode::ReadOnly,
                    approved_hosts,
                    file_root,
                },
                timeout_ms: timeout_ms(&action),
                action,
            },
            images,
        ))
    }
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
    let (command, shown) = {
        let builder = service.clone();
        let (call, request) = (call.clone(), request.clone());
        service
            .store
            .run_critical(
                StoreStep::run(call.batch.invocation_identity.agent_run_id, "browser.verify_start"),
                StoreAccess::Read,
                move || {
                    verifier.verify_source(&verified)?;
                    aura_persistence_v2::agent::checkpoint::verify_start_operation(
                        verifier.journal.lock()?.database.connection(),
                        operation,
                    )?;
                    builder.browser_command(&call, &request)
                },
            )
            .await?
    };
    let action = command.action.clone();
    // Stopping the turn, or cancelling the call, withdraws the command from the host.
    let stop = service
        .active_calls
        .session(call.snapshot.session_id)?
        .into_iter()
        .find(|entry| entry.call.validated.call.tool_call_id == call.validated.call.tool_call_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "browser.call_not_active: 这次浏览器调用在发给浏览器之前已经结束（回合停止了或调用被取消），什么都没做。"
            )
        })?
        .cancel
        .subscribe();
    // The call has started; a later permission change does not undo it.
    drop(start_lease);
    let answer = answer_of(&action, service.browser.send(command, stop).await, shown);
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
        domain_journal: json!({"schema":"agent-browser-v1","output":output,"error":answer.error}),
    };
    let owner = service.clone();
    let image = answer.image;
    service
        .store
        .run_critical(
            StoreStep::run(call.batch.invocation_identity.agent_run_id, "browser.result"),
            StoreAccess::Write,
            move || {
                Ok(owner.journal.lock()?.database.unit_of_work(|tx| {
                    let now = UtcTimestamp::now();
                    if let (Some(id), Some(image)) = (image_id, image.as_ref()) {
                        use base64::Engine as _;
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(&image.data_base64)
                            .map_err(|_| {
                                aura_persistence_core::PersistenceCoreError::Invalid(
                                    "browser screenshot",
                                )
                            })?;
                        let owner = request.operation.operation_id.to_string();
                        aura_persistence_core::artifact::put(
                            tx,
                            id,
                            &owner,
                            &image.media_type,
                            &bytes,
                            now,
                        )?;
                        // The conversation reads it back to send it to a model that takes
                        // images.
                        aura_persistence_core::artifact::grant_access(
                            tx,
                            id,
                            &owner,
                            &call.snapshot.session_id.to_string(),
                        )?;
                        // It is
                        // one of the run's own results, which the run may cite as evidence.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn host(outcome: BrowserHostOutcomeV1) -> Result<BrowserHostResultV1, BridgeError> {
        Ok(BrowserHostResultV1 {
            command_id: "bcm-1".into(),
            page_url: "about:blank".into(),
            document_id: None,
            outcome,
        })
    }

    fn read() -> BrowserHostActionV1 {
        BrowserHostActionV1::Snapshot { max_chars: 12_000 }
    }

    fn input() -> BrowserHostActionV1 {
        BrowserHostActionV1::Hover { element: "e1".into() }
    }

    fn said(answer: &Answer) -> &str {
        answer.error.as_deref().expect("a failure the model reads")
    }

    #[test]
    fn an_exception_in_the_page_is_told_in_a_sentence_with_whether_the_step_may_have_run() {
        let failed = BrowserHostOutcomeV1::Failed {
            message: "页面执行这一步（hover）时出错：Target closed。".into(),
            may_have_acted: true,
        };
        let answer = answer_of(&input(), host(failed), false);
        assert_eq!(
            said(&answer),
            format!("browser.failed: 页面执行这一步（hover）时出错：Target closed。{UNSURE}")
        );
        assert_eq!(answer.side_effect, SideEffectStateV1::Unknown);
        // An older host sent the page's bare text, on several lines.
        let bare = BrowserHostOutcomeV1::Failed {
            message: "Execution context was destroyed.\n    at Page.snapshot".into(),
            may_have_acted: false,
        };
        let answer = answer_of(&read(), host(bare), false);
        assert_eq!(
            said(&answer),
            format!("browser.failed: Execution context was destroyed。{UNCHANGED}")
        );
        assert_eq!(answer.side_effect, SideEffectStateV1::NoEffect);
    }

    #[test]
    fn a_report_aura_cannot_read_is_named_and_never_a_library_message_alone() {
        let unreadable = || BrowserHostOutcomeV1::Completed {
            result: serde_json::json!({"unexpected": true}),
            image: None,
        };
        let answer = answer_of(&read(), host(unreadable()), false);
        let text = said(&answer);
        assert!(text.starts_with("browser.failed: 浏览器返回的结果 Aura 读不懂（"), "{text}");
        assert!(text.ends_with(UNCHANGED), "{text}");
        assert_eq!(answer.side_effect, SideEffectStateV1::NoEffect);
        let answer = answer_of(&input(), host(unreadable()), false);
        let text = said(&answer);
        assert!(
            text.contains("这一步在浏览器里已经完成") && text.contains("不要直接重做"),
            "{text}"
        );
        assert_eq!(answer.side_effect, SideEffectStateV1::Unknown);
    }

    #[test]
    fn a_read_the_browser_did_not_answer_says_the_page_did_not_change() {
        let answer = answer_of(&read(), Err(BridgeError::TimedOut(20)), false);
        assert_eq!(
            said(&answer),
            format!("browser.no_answer: 浏览器在 20 秒内没有回应。{UNCHANGED}")
        );
        let answer = answer_of(&input(), Err(BridgeError::Interrupted), false);
        assert_eq!(said(&answer), format!("browser.interrupted: 浏览器连接中断。{UNSURE}"));
    }

    #[test]
    fn a_detail_from_outside_aura_is_one_bounded_line() {
        assert_eq!(super::super::outside_detail("first {x}\nsecond", 200), "first (x)");
        assert_eq!(super::super::outside_detail("a\u{7}b.", 200), "a b");
        assert_eq!(super::super::outside_detail("abcdef", 3), "abc…");
        assert_eq!(super::super::outside_detail("  \n", 200), "没有给出原因");
        assert_eq!(super::super::ended("没有回应"), "没有回应。");
        assert_eq!(super::super::ended("没有回应。"), "没有回应。");
    }
}
