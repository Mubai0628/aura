//! `computer.apps`: the catalog of programs with which have a window and which this conversation
//! may use, read from the desktop host before admission and answered as task state.
use super::super::*;
use super::resolve::OFF;
use aura_desktop_domain::{ComputerAppV1, ComputerAppsInput, ComputerAppsResult};
use serde_json::Value;

/// The most programs one listing returns.
const LISTED: usize = 200;
/// The most program names a search that found nothing names.
const NAMED: usize = 400;

/// Reads, before admission, what the step's computer calls need from the desktop: the catalog
/// their programs are named in, and for each `computer.apps` call the programs with their
/// windows. Nothing is read while computer control is off.
pub async fn prepare_before_admission(
    service: &AgentExecution,
    session: SessionId,
    mode: aura_desktop_domain::ComputerMode,
    calls: Vec<aura_persistence_v2::hooks::PendingToolCall>,
) {
    let Some(root) = service.desktop.get() else { return };
    let Ok(desktop) = root.select(mode) else { return };
    let settings = desktop.settings();
    if !settings.settings.enabled || settings.issue.is_some() {
        return;
    }
    let listings: Vec<_> = calls
        .iter()
        .filter(|call| call.tool_name == "computer.apps")
        .map(|call| call.tool_call_id)
        .collect();
    if listings.is_empty() {
        if calls.iter().any(|call| call.tool_name.starts_with("computer.")) {
            let _ = desktop.catalog().await;
        }
        super::safety::inspect(&desktop, session, &calls).await;
        return;
    }
    let listed = desktop.listed().await.map_err(|error| error.message);
    for call in listings {
        desktop.remember_listing(call, listed.clone());
    }
    super::safety::inspect(&desktop, session, &calls).await;
}

/// The listing for one `computer.apps` call, with what this conversation may use now.
pub(in crate::agent_execution) fn apps_result(
    service: &AgentExecution,
    call: &CallContext,
    connection: &aura_persistence_core::Connection,
    arguments: &Value,
) -> aura_persistence_core::StoreResult<Value> {
    let input: ComputerAppsInput = serde_json::from_value(arguments.clone())?;
    let session = call.snapshot.session_id;
    let empty = |notice: String| ComputerAppsResult { apps: vec![], more: 0, notice: Some(notice) };
    let selected = call
        .operation
        .content
        .identity
        .turn_task_id
        .ok_or_else(|| anyhow::anyhow!(OFF))
        .and_then(|turn| super::routing::for_turn(service, connection, turn));
    let result = match selected {
        Err(error) => empty(error.to_string()),
        Ok(desktop) => match desktop.listing(call.validated.call.tool_call_id) {
            None => empty(OFF.into()),
            Some(Err(message)) => {
                empty(format!("computer.unavailable: 读不到电脑上的程序：{message}"))
            }
            Some(Ok(listed)) => {
                let answered = aura_persistence_v2::desktop_apps::authorized(connection, session)?;
                let (found, notice) = search(listed, input.query.as_deref());
                let mut apps: Vec<ComputerAppV1> = found
                    .into_iter()
                    .map(|known| ComputerAppV1 {
                        allowed: answered.contains(&known.entry.id)
                            || desktop.always_allowed(session, &known.entry.id),
                        app: known.entry.id,
                        name: known.entry.name,
                        kind: known.entry.kind,
                        running: known.running,
                    })
                    .collect();
                let more = u32::try_from(apps.len().saturating_sub(LISTED)).unwrap_or(u32::MAX);
                apps.truncate(LISTED);
                ComputerAppsResult { apps, more, notice }
            }
        },
    };
    Ok(serde_json::to_value(result)?)
}

/// The programs whose name or id contains the query, ignoring case, those with a window first and
/// the catalog's order otherwise. When none does, the notice names every program: the catalog
/// names them in the computer's display language, which need not be the words of the request, and
/// the other computer tools take a name as well as an id, so the next call can open the program.
pub(super) fn search(
    mut listed: Vec<aura_desktop_service::Listed>,
    query: Option<&str>,
) -> (Vec<aura_desktop_service::Listed>, Option<String>) {
    listed.sort_by_key(|known| !known.running);
    let shown = query.map(str::trim).unwrap_or_default();
    let wanted = shown.to_lowercase();
    if wanted.is_empty() {
        return (listed, None);
    }
    let (found, others): (Vec<_>, Vec<_>) = listed.into_iter().partition(|known| {
        known.entry.name.to_lowercase().contains(&wanted)
            || known.entry.id.to_lowercase().contains(&wanted)
    });
    if !found.is_empty() || others.is_empty() {
        return (found, None);
    }
    let mut names: Vec<&str> = others.iter().map(|known| known.entry.name.as_str()).collect();
    let rest = names.len().saturating_sub(NAMED);
    names.truncate(NAMED);
    let rest = if rest == 0 { String::new() } else { format!(" 等，另有 {rest} 个") };
    let notice = format!(
        "没有名字或 id 含“{shown}”的程序。程序名按这台电脑的显示语言写，可能和请求里的说法不同；这里共有 {} 个程序：{}{rest}。其他电脑工具也认程序名。",
        others.len(),
        names.join("、"),
    );
    (found, Some(notice))
}
