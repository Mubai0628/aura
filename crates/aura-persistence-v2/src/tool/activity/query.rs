//! A question to the user, in its tool record — what was asked, the replies
//! the model offered, and how it ended (answered, declined, no response or cancelled), read from
//! the question's own record each time the record is shown.
use aura_agent_domain::*;
use aura_domain_common::{ToolCallId, TurnTaskId};
use aura_persistence_core::StoreResult;
use aura_tool_projection::AgentToolDisplay;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

pub(super) const NAME: &str = "query.user_question";

fn clipped(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push('…');
    }
    out
}

/// What the model asked and the replies it offered, from the call's own arguments.
pub(super) fn details(arguments: Option<&Value>) -> Vec<String> {
    let Some(input) = arguments
        .and_then(|value| serde_json::from_value::<QueryUserQuestionInput>(value.clone()).ok())
    else {
        return vec![];
    };
    let mut lines = vec![];
    for (index, question) in input.questions.iter().enumerate() {
        lines.push(format!("问题 {}：{}", index + 1, clipped(&question.text, 600)));
        lines.push(match &question.answer_type {
            QueryAnswerType::SingleChoice { options }
            | QueryAnswerType::MultiChoice { options } => {
                let labels =
                    options.iter().map(|option| clipped(&option.label, 200)).collect::<Vec<_>>();
                format!("提供的回复：{}", labels.join("；"))
            }
            QueryAnswerType::Confirm => "需要回答确认或不确认".to_owned(),
            QueryAnswerType::Text => "需要文字回答".to_owned(),
            QueryAnswerType::FileOrDirectory => "需要选择工作区里的文件或目录".to_owned(),
        });
    }
    lines
}

/// The question this call asked, in its latest state.
fn latest(db: &Connection, turn: TurnTaskId, call: ToolCallId) -> StoreResult<Option<UserQuery>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind='agent.query' \
             AND json_extract(payload,'$.tool_call_id')=?2 ORDER BY sequence DESC LIMIT 1",
            params![turn.to_string(), call.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|raw| serde_json::from_str::<UserQuery>(&raw).ok()))
}

/// Whether the question this call asked still waits for the user. Its call is then not finished
/// in the conversation either, so the record is read again until the question ends.
pub(super) fn waiting(db: &Connection, turn: TurnTaskId, call: ToolCallId) -> StoreResult<bool> {
    Ok(latest(db, turn, call)?.is_some_and(|query| query.state == UserQueryState::Pending))
}

/// How the question ended: its record's latest state.
pub(super) fn outcome(
    db: &Connection,
    turn: TurnTaskId,
    call: ToolCallId,
    view: &mut AgentToolDisplay,
) -> StoreResult<()> {
    let Some(query) = latest(db, turn, call)? else {
        return Ok(());
    };
    let answers = match &query.state {
        UserQueryState::Pending => {
            view.summary = "等待你的回答".into();
            return Ok(());
        }
        UserQueryState::Cancelled => {
            view.summary = "提问已取消".into();
            return Ok(());
        }
        UserQueryState::Answered { answers, .. } => answers,
    };
    if answers.iter().all(|answer| answer.value == QueryAnswerValue::NoResponse) {
        view.summary = "无人回应，已超时".into();
        view.details.push(
            "60 秒内没有操作这个提问。Aura 已告诉 AI：你可能在忙或不在电脑前，先做不受影响的工作，不替你决定，并在最后汇报里列出这个问题。"
                .into(),
        );
        return Ok(());
    }
    if answers.iter().all(|answer| answer.value == QueryAnswerValue::Declined) {
        view.summary = "你拒绝了回答".into();
        view.details.push("Aura 已告诉 AI：你不回答这些问题，不要再追问，也不要替你决定。".into());
        return Ok(());
    }
    view.summary = "已回答".into();
    for (index, question) in query.input.questions.iter().enumerate() {
        let Some(answer) = answers.iter().find(|answer| answer.question_id == question.id) else {
            continue;
        };
        view.details.push(format!("你的回答 {}：{}", index + 1, reply(question, &answer.value)));
    }
    Ok(())
}

fn reply(question: &QueryQuestion, value: &QueryAnswerValue) -> String {
    match value {
        QueryAnswerValue::Text { text } => clipped(text, 600),
        QueryAnswerValue::Custom { text } => format!("（自定义）{}", clipped(text, 600)),
        QueryAnswerValue::Confirm { accepted } => {
            if *accepted { "确认" } else { "不确认" }.to_owned()
        }
        QueryAnswerValue::Choices { ids } => {
            let options = match &question.answer_type {
                QueryAnswerType::SingleChoice { options }
                | QueryAnswerType::MultiChoice { options } => options.as_slice(),
                _ => &[],
            };
            ids.iter()
                .map(|id| {
                    options
                        .iter()
                        .find(|option| option.id == *id)
                        .map_or_else(|| id.clone(), |option| clipped(&option.label, 200))
                })
                .collect::<Vec<_>>()
                .join("；")
        }
        QueryAnswerValue::FileOrDirectory { .. } => "已选择工作区里的文件或目录".to_owned(),
        QueryAnswerValue::Declined => "拒绝回答".to_owned(),
        QueryAnswerValue::NoResponse => "无人回应".to_owned(),
    }
}
