//! A new conversation from one finished turn of another.
//!
//! The conversation is created in the primary registry in the source's Workspace with the
//! source's permission mode. The execution store then carries the source's history into it in one
//! transaction. When that transaction fails, the new conversation is deleted again, so no half
//! branch is left and the user can simply try again.
use crate::agent_store::StoreError;
use crate::*;
use aura_agent_domain::{AgentSessionBranchRequest, AgentSessionBranchResult};
use aura_persistence_v2::agent::branch::BranchImport;
use aura_workspace_domain::SessionCreateRequest;

const TITLE_PREFIX: &str = "分支：";
/// The primary registry's limit on a conversation's title, in characters.
const TITLE_LIMIT: usize = 200;

pub(crate) async fn dispatch(
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> anyhow::Result<serde_json::Value> {
    let command = request
        .command_invocation_id
        .ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?;
    let input: AgentSessionBranchRequest = serde_json::from_value(request.params.clone())?;
    let started = std::time::Instant::now();
    let persistence = Arc::clone(&services.persistence);
    let source = registry(&persistence, |p| p.workspace_session_projection())
        .await?
        .sessions
        .into_iter()
        .find(|s| s.session_id == input.session_id)
        .ok_or_else(|| anyhow::anyhow!("session.not_found: 这个会话已不存在。"))?;
    let session = source.session_id;
    let mode = registry(&persistence, move |p| p.session_permission_state(session)).await?.mode;
    let create = SessionCreateRequest {
        workspace_id: source.workspace_id,
        title: branch_title(&source.title),
    };
    let correlation = request.id;
    let created =
        registry(&persistence, move |p| p.create_session(command, create, mode, correlation))
            .await?;
    let created_ms = started.elapsed().as_millis();
    let import = BranchImport {
        branch: created.session_id,
        source: source.session_id,
        source_title: source.title.clone(),
        turn_task_id: input.turn_task_id,
        command_invocation_id: command,
        at: UtcTimestamp::now(),
    };
    let carried = services.agent()?.carry_into_branch(import).await;
    let origin = match carried {
        Ok(Ok(origin)) => origin,
        // The store did not answer in time: the branch may still appear, so nothing is removed.
        Ok(Err(error @ StoreError::Unknown { .. })) => {
            append_core_log(
                "session.branch_unknown",
                json!({"session_id": created.session_id, "reason": error.code()}),
            );
            anyhow::bail!(
                "agent.branch_unknown: 新建分支还没有答复，可能仍在进行；请稍后在侧边栏查看，不要重复新建。"
            )
        }
        failed => {
            // No half branch: the conversation made for it goes again.
            if let Err(cleanup) =
                crate::session_lifecycle::Purger::of(services).delete(created.session_id).await
            {
                append_core_log(
                    "session.branch_cleanup_failed",
                    json!({"session_id": created.session_id, "reason": cleanup.to_string()}),
                );
            }
            return Err(refusal(failed));
        }
    };
    // The whole command's time beside the import's own (`session.branch_timing`).
    append_core_log(
        "session.branch_steps",
        json!({"created_ms": created_ms, "total_ms": started.elapsed().as_millis()}),
    );
    Ok(serde_json::to_value(AgentSessionBranchResult {
        session_id: created.session_id,
        workspace_id: created.workspace_id,
        title: created.title,
        origin,
    })?)
}

/// "分支：〈原标题〉", within the registry's title limit. An untitled conversation is "新会话", as the
/// sidebar names it.
fn branch_title(source: &str) -> String {
    let title = if source.trim().is_empty() { "新会话" } else { source.trim() };
    format!("{TITLE_PREFIX}{title}").chars().take(TITLE_LIMIT).collect()
}

/// Why a branch was not made, in the user's words.
fn refusal(
    failed: anyhow::Result<Result<aura_agent_domain::AgentBranchOrigin, StoreError>>,
) -> anyhow::Error {
    let reason = match failed {
        Ok(Err(StoreError::Failed { error, .. })) => error.to_string(),
        Ok(Err(error)) => error.code().to_owned(),
        Err(error) => error.to_string(),
        Ok(Ok(_)) => String::new(),
    };
    if reason.contains("agent.branch_turn_unfinished") {
        anyhow::anyhow!("agent.branch_turn_unfinished: 只能从已经完成回答的一轮新建分支。")
    } else if reason.contains("agent.branch_turn_unknown") {
        anyhow::anyhow!("agent.branch_turn_unknown: 这一轮不在这个会话里，请刷新后再试。")
    } else {
        // The store's own words stay in the log; the user is told what happened and what to do.
        append_core_log("session.branch_failed", json!({"reason": reason}));
        anyhow::anyhow!("agent.branch_failed: 没能新建分支，没有留下半个分支，可以重试。")
    }
}

async fn registry<T: Send + 'static>(
    persistence: &Arc<PersistenceCoordinator>,
    work: impl FnOnce(&PersistenceCoordinator) -> Result<T, aura_persistence::PersistenceError>
    + Send
    + 'static,
) -> anyhow::Result<T> {
    let persistence = Arc::clone(persistence);
    Ok(tokio::task::spawn_blocking(move || work(&persistence)).await??)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_branch_is_titled_after_its_source_within_the_limit() {
        assert_eq!(super::branch_title("修复登录"), "分支：修复登录");
        assert_eq!(super::branch_title("  "), "分支：新会话");
        let long = "长".repeat(300);
        assert_eq!(super::branch_title(&long).chars().count(), 200);
    }
}
