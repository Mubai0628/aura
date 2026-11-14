//! The words a terminal call's journal gives the model when the call fails.
use aura_execution_contract::SideEffectStateV1;
use aura_persistence_core::PersistenceCoreError;
use aura_persistence_v2::tool::refusal_text;

const NOT_STARTED: &str = "terminal.not_started";
const LOOK_FIRST: &str = "请先用 terminal.read 查看终端现状再决定；不要自动重放输入。";

/// The journal's `error` for a terminal call that did not succeed: a `<code>: <sentence>` the
/// terminal wrote is kept as it is; any other cause is worded with what the call did, never as a
/// Rust value or a bare internal code.
pub(super) fn error_text(error: &anyhow::Error, side_effect: SideEffectStateV1) -> String {
    let text = error.to_string();
    if worded(&text) {
        return text;
    }
    let code = leading_code(&text);
    match side_effect {
        SideEffectStateV1::NoEffect | SideEffectStateV1::NotStarted => {
            let chain = error.chain().map(ToString::to_string).collect::<Vec<_>>().join(": ");
            if let Some(sentence) = refusal_text::known(&chain) {
                return format!("{}: {sentence}", code.unwrap_or(NOT_STARTED));
            }
            if matches!(
                error.downcast_ref::<PersistenceCoreError>(),
                Some(PersistenceCoreError::Conflict)
            ) {
                return format!(
                    "{NOT_STARTED}: 这次调用所属的轮次在终端操作开始前已经停止或改变，没有执行。需要时请重新调用。"
                );
            }
            if error.is::<serde_json::Error>() {
                return format!(
                    "terminal.invalid_arguments: 这次终端调用的参数无法解析（{}），没有执行。请按工具说明改正参数后再试。",
                    bounded(&text)
                );
            }
            format!(
                "{}: Aura 内部出错，这次终端操作没有执行，没有改变任何东西。请再试一次；仍然失败时请告诉用户。{}",
                code.unwrap_or(NOT_STARTED),
                detail(&text)
            )
        }
        SideEffectStateV1::Compensated => format!(
            "terminal.open_rolled_back: 终端已经打开，但 Aura 没能记下它，已经把它关闭，没有留下运行中的终端。请再试一次；仍然失败时请告诉用户。{}",
            detail(&text)
        ),
        SideEffectStateV1::Unknown => format!(
            "terminal.outcome_unknown: 终端操作开始后出错，Aura 无法确认它已经做了什么（终端可能已经打开，输入可能已经送达）。{LOOK_FIRST}{}",
            detail(&text)
        ),
        _ => format!(
            "terminal.result_unavailable: 终端操作已经执行，但之后 Aura 没能取回它的结果。请用 terminal.read 查看终端现状。{}",
            detail(&text)
        ),
    }
}

/// `<code>: <sentence>`: a line written for the model.
fn worded(text: &str) -> bool {
    let Some((code, rest)) = text.lines().next().unwrap_or_default().split_once(':') else {
        return false;
    };
    let rest = rest.trim();
    is_code(code)
        && rest.chars().count() >= 8
        && (rest.contains(' ') || rest.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch)))
}

/// The code a bare or `<code>: …` reason starts with.
fn leading_code(text: &str) -> Option<&str> {
    let code = text.split([':', ';']).next()?.trim();
    is_code(code).then_some(code)
}

fn is_code(token: &str) -> bool {
    token.len() <= 96
        && token.contains('.')
        && token.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        })
}

/// Library or system text after the sentence: never a bare code or a Rust value.
fn detail(text: &str) -> String {
    if leading_code(text) == Some(text.trim()) || text.contains(['{', '}']) || text.is_empty() {
        return String::new();
    }
    format!("细节：{}", bounded(text))
}

/// Text shown after a sentence: its start, on one line.
fn bounded(text: &str) -> String {
    let mut shown: String =
        text.chars().take(200).map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    if text.chars().count() > 200 {
        shown.push('…');
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_sentence_reaches_the_model_unchanged() {
        let line =
            "terminal.ended: 终端已结束，这次没有输入任何内容。请调用 terminal.open 新建终端。";
        assert_eq!(error_text(&anyhow::anyhow!(line), SideEffectStateV1::NoEffect), line);
        let prompt = "terminal.sensitive.prompt_gone: the terminal no longer shows the prompt; nothing was typed";
        assert_eq!(error_text(&anyhow::anyhow!(prompt), SideEffectStateV1::NoEffect), prompt);
    }

    #[test]
    fn a_bare_code_is_worded_with_what_the_call_did() {
        let said = error_text(&anyhow::anyhow!("terminal.owner_lock"), SideEffectStateV1::NoEffect);
        assert!(said.starts_with("terminal.owner_lock: Aura 内部出错"), "{said}");
        assert!(said.contains("没有执行") && !said.contains("细节"), "{said}");

        let unknown =
            error_text(&anyhow::anyhow!("pipe closed (os error 232)"), SideEffectStateV1::Unknown);
        assert!(unknown.starts_with("terminal.outcome_unknown: "), "{unknown}");
        assert!(unknown.contains("terminal.read") && unknown.contains("os error 232"), "{unknown}");
        assert!(!unknown.contains("没有执行"), "{unknown}");

        let debug = error_text(&anyhow::anyhow!("Thing {{ id: 1 }}"), SideEffectStateV1::NoEffect);
        assert!(debug.starts_with("terminal.not_started: ") && !debug.contains("Thing"), "{debug}");
    }

    #[test]
    fn a_stopped_turn_and_a_scope_change_are_said_as_not_started() {
        let conflict = anyhow::Error::from(PersistenceCoreError::Conflict);
        let said = error_text(&conflict, SideEffectStateV1::NoEffect);
        assert!(said.starts_with("terminal.not_started: ") && !said.contains("conflict"), "{said}");
        let scope = error_text(
            &anyhow::anyhow!("agent.source_or_scope_changed"),
            SideEffectStateV1::NoEffect,
        );
        assert!(scope.starts_with("agent.source_or_scope_changed: "), "{scope}");
    }
}
