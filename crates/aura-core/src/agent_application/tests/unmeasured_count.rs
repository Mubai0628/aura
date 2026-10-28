//! A model no provider has counted yet. Its first request is counted by character class, so a
//! message whose bytes pass the window's input budget but whose text does not is sent, not held
//! for the user. A provider that still refuses such a request as too long has the request rebuilt
//! smaller; the refusal is never taken to mean that the model's window is wrong.
use super::l10_15_compaction::{call, first_request_tokens, start};
use super::*;
use aura_context_domain::AgentContextRemediationView;
use aura_model_domain::ContextWindowResolution;

/// About 245 KB of records: past the fixture window's input budget of 239,104 tokens byte for
/// byte, about a third of it by character class.
fn records() -> String {
    let mut text = String::from("AURA-UNMEASURED: reply briefly.\n");
    for n in 1.. {
        if text.len() >= 245_000 {
            break;
        }
        text.push_str(&format!(
            "Record {n:05}: sensor {} at station {} read {} units.\n",
            n % 97,
            n * 7 % 113,
            n * 37 % 1000
        ));
    }
    text
}

fn remediation(value: Value) -> AgentContextRemediationView {
    serde_json::from_value(value).expect("remediation view")
}

#[tokio::test]
async fn a_first_message_past_the_budget_in_bytes_is_sent_when_its_text_fits() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::ReadOnly).await;
    let text = records();
    let run = start(&runtime, session, model, &text).await;
    waiting::wait(&runtime, &run, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let counted = first_request_tokens(&runtime, session).await;
    let sent = provider.raw_requests.lock().expect("requests")[0].len() as u64;
    assert!(
        counted < sent / 2,
        "counted by character class: {counted} for a request of {sent} bytes"
    );
}

/// The same message, counted byte for byte as every uncounted request once was, never reaches
/// the provider: the user is asked to reduce it although the model would have taken it.
#[tokio::test]
async fn the_same_message_counted_byte_for_byte_waits_for_the_user() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::ReadOnly).await;
    count_by_bytes(model);
    let run = start(&runtime, session, model, &records()).await;
    waiting::wait(&runtime, &run, |v| {
        matches!(v.run.state, AgentRunState::AwaitingContextRemediation { .. })
    })
    .await;
    let view = remediation(call(&runtime, "agent.context.remediation", json!(run)).await);
    let plan = view.remediation.expect("the reasons").plan;
    assert_eq!(plan.overflow.error_code, "context.non_compressible_overflow");
    assert!(provider.requests.lock().expect("requests").is_empty(), "nothing was sent");
}

/// The provider finds a request counted by the rule too long. Nothing is wrong with the model's
/// window, so the user is not asked to correct it: the request is rebuilt, and with nothing to
/// compact the user is told the provider refused it.
#[tokio::test]
async fn a_refused_request_counted_by_the_rule_is_rebuilt_not_taken_for_a_wrong_window() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Window { failures: 1 }).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::ReadOnly).await;
    count_by_character_class(model);
    let run = start(&runtime, session, model, "AURA-UNMEASURED-REFUSED: reply briefly").await;
    waiting::wait(&runtime, &run, |v| {
        matches!(v.run.state, AgentRunState::AwaitingContextRemediation { .. })
    })
    .await;
    let view = remediation(call(&runtime, "agent.context.remediation", json!(run)).await);
    let correction = view.provider_correction.expect("the provider's refusal is recorded");
    assert!(
        matches!(
            correction.correction.resolution,
            ContextWindowResolution::P06RebuildRequired { .. }
        ),
        "rebuilt, not a question about the window: {:?}",
        correction.correction.resolution
    );
    assert_ne!(correction.state, aura_model_domain::AgentContextWindowState::Awaiting);
    let plan = view.remediation.expect("the user decides").plan;
    assert_eq!(plan.overflow.error_code, "context.provider_rebuild_no_progress");
    let settings = call(&runtime, "models.settings.get_snapshot", json!({})).await;
    let context = &settings["registry"]["models"][model.to_string()]["context"];
    assert!(context.is_object(), "the model's window settings: {settings}");
    assert_eq!(
        context["runtime_corrected_window_tokens"],
        Value::Null,
        "the model's window is left as configured"
    );
    assert_eq!(provider.requests.lock().expect("requests").len(), 1, "never sent again as it was");
}
