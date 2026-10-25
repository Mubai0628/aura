//! How a question to the user ends, and what the model reads about it.
//!
//! A question nobody touches for [`QUERY_IDLE_SECONDS`] shows a countdown of
//! [`QUERY_COUNTDOWN_SECONDS`]; when that runs out the question ends as "no response". The model
//! then reads that the user may be busy or away, that this is not an answer, and that it should
//! go on with what the question does not affect without deciding for the user. Every reply names
//! the question and the chosen labels, not only option ids.
use crate::{QueryAnswer, QueryAnswerType, QueryAnswerValue, QueryUserQuestionInput};
use aura_domain_common::{UserQueryId, UtcTimestamp};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// How long a question may go untouched before its countdown shows.
pub const QUERY_IDLE_SECONDS: u64 = 30;
/// How long the countdown runs before the question ends as "no response".
pub const QUERY_COUNTDOWN_SECONDS: u64 = 30;

/// When a question ends as "no response": its idle time and its countdown after it was asked, or
/// after the user last touched it.
#[must_use]
pub fn query_deadline(created_at: UtcTimestamp, last_touch: Option<UtcTimestamp>) -> UtcTimestamp {
    let from = last_touch.map_or(created_at, |touch| touch.max(created_at));
    UtcTimestamp::from_value(
        from.value() + std::time::Duration::from_secs(QUERY_IDLE_SECONDS + QUERY_COUNTDOWN_SECONDS),
    )
}

const DECLINED_NOTE: &str = "The user chose not to answer these questions. Do not ask them again in this turn and do not decide them for the user. Continue with the work that does not depend on the answers, and say in your final report what was left undone because of it.";
const NO_RESPONSE_NOTE: &str = "No response: nobody touched these questions for 60 seconds, so the user may be busy or away from the computer. This is not an answer and not a refusal. Do not decide for the user. Continue any work these questions do not affect, leave the decision open, and list these questions in your final report as still waiting for the user.";

/// The text the model reads for a question's end, as JSON. `files` holds the workspace path of
/// each question answered with a file or directory.
#[must_use]
pub fn query_reply(
    query_id: UserQueryId,
    input: &QueryUserQuestionInput,
    answers: &[QueryAnswer],
    files: &BTreeMap<String, String>,
) -> String {
    let outcome = if answers.iter().all(|a| a.value == QueryAnswerValue::NoResponse) {
        "no_response"
    } else if answers.iter().all(|a| a.value == QueryAnswerValue::Declined) {
        "declined"
    } else {
        "answered"
    };
    let replies = input
        .questions
        .iter()
        .filter_map(|question| {
            let answer = answers.iter().find(|a| a.question_id == question.id)?;
            Some(json!({
                "question_id": question.id,
                "question": question.text,
                "reply": reply(&question.answer_type, &answer.value, files.get(&question.id)),
            }))
        })
        .collect::<Vec<_>>();
    let mut body = json!({ "query_id": query_id, "outcome": outcome, "answers": replies });
    match outcome {
        "no_response" => body["note"] = Value::from(NO_RESPONSE_NOTE),
        "declined" => body["note"] = Value::from(DECLINED_NOTE),
        _ => {}
    }
    json!({ "query_reply": body }).to_string()
}

fn reply(kind: &QueryAnswerType, value: &QueryAnswerValue, path: Option<&String>) -> Value {
    match value {
        QueryAnswerValue::Text { text } => json!({ "kind": "text", "text": text }),
        QueryAnswerValue::Choices { ids } => {
            let options = match kind {
                QueryAnswerType::SingleChoice { options }
                | QueryAnswerType::MultiChoice { options } => options.as_slice(),
                _ => &[],
            };
            let chosen = ids
                .iter()
                .map(|id| {
                    let label = options.iter().find(|o| o.id == *id).map(|o| o.label.as_str());
                    json!({ "id": id, "label": label })
                })
                .collect::<Vec<_>>();
            json!({ "kind": "choices", "chosen": chosen })
        }
        QueryAnswerValue::Confirm { accepted } => {
            json!({ "kind": "confirm", "accepted": accepted })
        }
        QueryAnswerValue::FileOrDirectory { file_reference_id } => json!({
            "kind": "file_or_directory",
            "file_reference_id": file_reference_id,
            "relative_path": path,
        }),
        QueryAnswerValue::Custom { text } => json!({ "kind": "custom", "text": text }),
        QueryAnswerValue::Declined => json!({ "kind": "declined" }),
        QueryAnswerValue::NoResponse => json!({ "kind": "no_response" }),
    }
}
