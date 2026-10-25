//! Every question takes the user's own words, the user may decline, and a
//! question nobody touches ends as "no response". What the model reads says which, with the
//! questions and the chosen labels, never only option ids.
use aura_agent_domain::*;
use aura_domain_common::*;

fn choice(id: &str, label: &str) -> QueryChoice {
    QueryChoice { id: id.into(), label: label.into() }
}

fn input() -> QueryUserQuestionInput {
    QueryUserQuestionInput {
        questions: vec![
            QueryQuestion {
                id: "empty_workspace".into(),
                text: "工作区是空的，你希望我怎么处理？".into(),
                answer_type: QueryAnswerType::SingleChoice {
                    options: vec![choice("minimal", "创建最小 AGENTS.md"), choice("skip", "跳过")],
                },
            },
            QueryQuestion {
                id: "proceed".into(),
                text: "现在继续吗？".into(),
                answer_type: QueryAnswerType::Confirm,
            },
        ],
    }
}

fn answers(values: [QueryAnswerValue; 2]) -> Vec<QueryAnswer> {
    let [first, second] = values;
    vec![
        QueryAnswer { question_id: "empty_workspace".into(), value: first },
        QueryAnswer { question_id: "proceed".into(), value: second },
    ]
}

#[test]
fn every_question_takes_the_users_own_words() {
    let input = input();
    let custom = answers([
        QueryAnswerValue::Custom { text: "先列出目录再决定".into() },
        QueryAnswerValue::Custom { text: "等我看完再说".into() },
    ]);
    input.validate_answers(&custom).expect("a custom reply answers any question");
    let blank = answers([
        QueryAnswerValue::Custom { text: "   ".into() },
        QueryAnswerValue::Confirm { accepted: true },
    ]);
    assert!(input.validate_answers(&blank).is_err(), "a custom reply has words");
    let long = answers([
        QueryAnswerValue::Custom { text: "长".repeat(MAX_CUSTOM_REPLY_CHARS + 1) },
        QueryAnswerValue::Confirm { accepted: true },
    ]);
    assert!(input.validate_answers(&long).is_err(), "a custom reply is bounded");
}

#[test]
fn declining_and_no_response_cover_the_whole_question() {
    let input = input();
    input
        .validate_answers(&answers([QueryAnswerValue::Declined, QueryAnswerValue::Declined]))
        .expect("declined");
    input
        .validate_answers(&answers([QueryAnswerValue::NoResponse, QueryAnswerValue::NoResponse]))
        .expect("no response");
    for mixed in [
        answers([QueryAnswerValue::Declined, QueryAnswerValue::Confirm { accepted: true }]),
        answers([QueryAnswerValue::NoResponse, QueryAnswerValue::Declined]),
    ] {
        assert!(input.validate_answers(&mixed).is_err(), "{mixed:?}");
    }
}

#[test]
fn the_model_reads_the_questions_and_the_chosen_labels() {
    let reply = query_reply(
        UserQueryId::new(),
        &input(),
        &answers([
            QueryAnswerValue::Choices { ids: vec!["minimal".into()] },
            QueryAnswerValue::Custom { text: "先别提交".into() },
        ]),
        &Default::default(),
    );
    let value: serde_json::Value = serde_json::from_str(&reply).expect("JSON");
    let body = &value["query_reply"];
    assert_eq!(body["outcome"], "answered");
    assert!(body.get("note").is_none(), "{reply}");
    assert_eq!(body["answers"][0]["question"], "工作区是空的，你希望我怎么处理？");
    assert_eq!(body["answers"][0]["reply"]["chosen"][0]["label"], "创建最小 AGENTS.md");
    assert_eq!(body["answers"][1]["reply"]["kind"], "custom");
    assert_eq!(body["answers"][1]["reply"]["text"], "先别提交");
}

#[test]
fn no_response_tells_the_model_not_to_decide_and_to_carry_on() {
    let reply = query_reply(
        UserQueryId::new(),
        &input(),
        &answers([QueryAnswerValue::NoResponse, QueryAnswerValue::NoResponse]),
        &Default::default(),
    );
    let value: serde_json::Value = serde_json::from_str(&reply).expect("JSON");
    let body = &value["query_reply"];
    assert_eq!(body["outcome"], "no_response");
    let note = body["note"].as_str().expect("note");
    for words in ["not an answer", "Do not decide", "final report", "busy or away"] {
        assert!(note.contains(words), "{note}");
    }
    let declined = query_reply(
        UserQueryId::new(),
        &input(),
        &answers([QueryAnswerValue::Declined, QueryAnswerValue::Declined]),
        &Default::default(),
    );
    let value: serde_json::Value = serde_json::from_str(&declined).expect("JSON");
    assert_eq!(value["query_reply"]["outcome"], "declined");
    assert!(value["query_reply"]["note"].as_str().is_some_and(|note| note.contains("chose not")));
}

#[test]
fn a_question_left_alone_ends_after_its_idle_time_and_its_countdown() {
    assert_eq!(QUERY_IDLE_SECONDS, 30);
    assert_eq!(QUERY_COUNTDOWN_SECONDS, 30);
    let created: UtcTimestamp = "2026-10-20T10:00:00Z".parse().expect("time");
    let touched: UtcTimestamp = "2026-10-20T10:00:45Z".parse().expect("time");
    assert_eq!(query_deadline(created, None).to_string(), "2026-10-20T10:01:00Z");
    assert_eq!(query_deadline(created, Some(touched)).to_string(), "2026-10-20T10:01:45Z");
}
