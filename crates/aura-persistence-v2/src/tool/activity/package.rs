//! Package tool facts the session row states from stored execution facts: where the call ran,
//! and that a call allowed to finish began before its package was disabled, untrusted,
//! uninstalled or replaced. The Renderer does not infer either.
use aura_execution_contract::{AttemptState, ExecutionAttempt, SecuredOperation};
use aura_persistence_core::{StoreResult, record};
use aura_security_contract::{PackageRuntimeClass, ResourceDescriptor};
use aura_tool_projection::AgentToolDisplay;
use rusqlite::Connection;

/// The warning Core records on a package call that started before its package changed.
const CHANGED_DURING_CALL: &str = "tool.package.changed_during_call";
pub(super) const PROGRAM_NOTICE: &str = "以当前 Windows 用户身份运行，Aura 不隔离它的文件写入";
pub(super) const HTTP_NOTICE: &str = "由 Aura 代为发出 HTTPS 请求，只访问工具声明的地址";
pub(super) const CHANGED_NOTICE: &str = "开始后工具包已变更";
pub(super) const CHANGED_DETAIL: &str =
    "这个调用在工具包停用、撤销信任、卸载或更换版本之前已开始，已允许它完成；之后的调用会被拒绝。";

pub(super) fn decorate(
    db: &Connection,
    view: AgentToolDisplay,
    operation: Option<&SecuredOperation>,
) -> StoreResult<AgentToolDisplay> {
    let Some(operation) = operation else {
        return Ok(view);
    };
    let Some(runtime) = operation.content.resources.iter().find_map(|resource| match resource {
        ResourceDescriptor::ToolPackage { runtime, .. } => Some(*runtime),
        _ => None,
    }) else {
        return Ok(view);
    };
    let mut changed = false;
    let mut attempts = db
        .prepare("SELECT payload FROM execution_attempts WHERE operation_id=?1 ORDER BY ordinal")?;
    let payloads = attempts
        .query_map([operation.operation_id.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    // A call refused before dispatch never ran anywhere; its row states the refusal alone.
    if payloads.is_empty() {
        return Ok(view);
    }
    for payload in payloads {
        let attempt: ExecutionAttempt = record::decode(&payload)?;
        if let AttemptState::Terminal { result } = attempt.state {
            changed |= result.warnings.iter().any(|warning| warning == CHANGED_DURING_CALL);
        }
    }
    let mut view = apply(view, runtime, changed);
    if runtime == PackageRuntimeClass::Http {
        super::network_route::append_route_detail(db, operation.operation_id, &mut view);
    }
    Ok(view)
}

fn append(view: &mut AgentToolDisplay, text: &str) {
    if !view.summary.is_empty() {
        view.summary.push('；');
    }
    view.summary.push_str(text);
}

fn apply(
    mut view: AgentToolDisplay,
    runtime: PackageRuntimeClass,
    changed: bool,
) -> AgentToolDisplay {
    match runtime {
        PackageRuntimeClass::Program => append(&mut view, PROGRAM_NOTICE),
        PackageRuntimeClass::Http => view.details.push(HTTP_NOTICE.into()),
    }
    if changed {
        append(&mut view, CHANGED_NOTICE);
        view.details.push(CHANGED_DETAIL.into());
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> AgentToolDisplay {
        AgentToolDisplay {
            title: "user.org.example.tool".into(),
            target: None,
            summary: "执行成功".into(),
            details: vec![],
            truncated: false,
            internal: false,
            image: None,
            offer: None,
        }
    }

    #[test]
    fn a_program_states_it_ran_as_the_user_and_http_states_aura_sent_it() {
        let program = apply(view(), PackageRuntimeClass::Program, false);
        assert_eq!(program.summary, format!("执行成功；{PROGRAM_NOTICE}"));
        assert!(program.details.is_empty());
        let http = apply(view(), PackageRuntimeClass::Http, false);
        assert_eq!(http.summary, "执行成功");
        assert_eq!(http.details, vec![HTTP_NOTICE.to_owned()]);
    }

    #[test]
    fn a_call_that_outlived_its_package_says_so_in_the_row_and_the_details() {
        let changed = apply(view(), PackageRuntimeClass::Program, true);
        assert!(changed.summary.ends_with(CHANGED_NOTICE), "{}", changed.summary);
        assert_eq!(changed.details, vec![CHANGED_DETAIL.to_owned()]);
        let http = apply(view(), PackageRuntimeClass::Http, true);
        assert_eq!(http.summary, format!("执行成功；{CHANGED_NOTICE}"));
        assert_eq!(http.details, vec![HTTP_NOTICE.to_owned(), CHANGED_DETAIL.to_owned()]);
    }
}
