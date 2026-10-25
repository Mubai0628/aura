//! R4: the real Core `prepare → authorisation → transport` body, captured from the wire.
//!
//! WHY THIS MODULE EXISTS, AND WHAT IT IS NOT
//! C's earlier evidence compiled a **hardcoded** `ACTIVITY_BODY` constant and asserted the three
//! adapters placed it correctly. That is adapter-compile-layer evidence: it proves the adapters do
//! the right thing with a body they are *given*, and says nothing about whether Core ever produces
//! such a body on the production path. The brief is explicit that the 9/9 must not be presented as
//! the production chain.
//!
//! This module closes that gap the only way it can be closed honestly: by driving the real Agent
//! through the real context preparation and the real transport, against a **local HTTP fixture that
//! really receives the request**, and then reading the bytes out of the fixture's own capture. The
//! only substituted component is the remote Provider's answer, which is what a fixture is for.
//!
//! WHAT IS MEASURED
//! * the activity fragment is really present in a request Core sent, not in one a test assembled;
//! * its identity is the frozen snapshot's (`sampled_at`/`revision`/coverage from the real index),
//!   so the body a Provider received is the body the Inspector described;
//! * a **retry** re-sends the same frozen bytes rather than re-sampling, which is the property the
//!   "one consistent picture per step" rule exists for;
//! * and the twelve sections and the tool batch are unchanged by any of it.
use super::provider_fixture::{Provider, Scenario};
use super::waiting;
use super::*;

/// The marker every rendered activity fragment carries, so a captured body can be identified as
/// carrying one without depending on any particular row.
const FRAGMENT_MARKER: &str = "coverage=";

/// Every captured request body, as the fixture really received it.
fn captured(provider: &Provider) -> Vec<serde_json::Value> {
    provider.requests.lock().expect("the fixture's captured requests").clone()
}

/// The exact dynamic-tail text of one captured body, for each of the adapters.
///
/// The three adapters place the dynamic tail in genuinely different places, and the capture corrected
/// an assumption here too: on the Responses wire the trailing `developer` input item's `content` is an
/// **array of parts**, not a string, so a reader that accepted only a string found no fragment in a
/// body that really carried one. Each shape is now read as it really is.
fn dynamic_tail(body: &serde_json::Value) -> String {
    let parts_to_text = |content: &serde_json::Value| -> String {
        if let Some(text) = content.as_str() {
            return text.to_owned();
        }
        content
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| part["text"].as_str().or_else(|| part.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    // OpenAI Chat: the trailing `system`/`developer` message after the history.
    if let Some(messages) = body["messages"].as_array()
        && let Some(last) =
            messages.iter().rev().find(|m| m["role"] == "system" || m["role"] == "developer")
    {
        return parts_to_text(&last["content"]);
    }
    // OpenAI Responses: the trailing `developer` input item, whose content is a part array.
    if let Some(input) = body["input"].as_array()
        && let Some(item) =
            input.iter().rev().find(|item| item["role"] == "developer" || item["role"] == "system")
    {
        return parts_to_text(&item["content"]);
    }
    // Anthropic: the trailing system string.
    if let Some(system) = body["system"].as_str() {
        return system.to_owned();
    }
    String::new()
}

/// **R4** — a real model step's Provider request really carries the activity fragment.
///
/// # The red line this satisfies
///
/// The harness must **really capture the bytes Core sent**. This reads them out of the local HTTP
/// fixture's own record of the requests it received, so a body that never reached the wire cannot
/// satisfy the assertion. A test that assembled its own request body would pass without Core ever
/// producing one, which is exactly the substitution the brief forbids.
///
/// # What it rejects
///
/// * an implementation that builds the fragment but never places it in the outgoing request;
/// * a fragment whose identity does not come from the frozen snapshot (the header must name the
///   real revision and coverage, which only the index's own sampling produces);
/// * and a fragment placed in the stable prefix rather than the dynamic tail.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_provider_request_really_carries_the_activity_fragment() {
    let host = crate::r03_tests::Fixture::new();
    // `Write` drives a real `fs.write` through the model loop, so the run's own activity is
    // published by the real Agent dispatch before the following step's context is prepared.
    // It runs in the mode that authorizes that write: the lower modes now *ask* instead of
    // refusing, and a test about the activity fragment is not about the approval boundary.
    let provider = Provider::scenario(Scenario::Write).await;
    let (runtime, start) = waiting::begin_writing(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    // Wait for the run to reach a terminal state, so every request of this run has been sent.
    waiting::wait(&runtime, &request, |p| p.turn.state.terminal()).await;

    let bodies = captured(&provider);
    assert!(
        bodies.len() >= 2,
        "the real chain really asked the Provider more than once (a tool step and a continuation): {}",
        bodies.len()
    );

    // THE DECISIVE FACT: a request Core really sent carries a rendered activity fragment.
    //
    // The marker is the fragment's own header (`coverage=`), which only `rendered_body()` emits. An
    // earlier version of this test also required the literal `agent.activity_index.v1` in the body;
    // that was **my** error, not the product's. That string is the fragment's `ContextSourceRef`
    // state kind - a manifest and compaction identity - and it is deliberately not part of the
    // rendered bytes a model reads. The real capture below is what corrected the assertion, which is
    // the point of reading the bytes instead of assuming their shape.
    let carrier = bodies
        .iter()
        .enumerate()
        .find(|(_, body)| dynamic_tail(body).contains(FRAGMENT_MARKER))
        .map(|(index, body)| (index, dynamic_tail(body)))
        .expect(
            "a real Provider request must carry the activity fragment in its dynamic tail; the \
             hardcoded-body adapter tests cannot establish this because they never involve Core",
        );

    let (ordinal, tail) = carrier;
    // The header is the frozen snapshot's own facts, so this is Core's sampling rather than a
    // string a test could have written. The captured bytes really read
    // `sampled_at=… runtime=run_… revision=5 coverage=complete in_scope=0 shown=0 omitted=0`.
    assert!(
        tail.contains("sampled_at=") && tail.contains("runtime=") && tail.contains("revision="),
        "the fragment carries the frozen snapshot's identity: {tail}"
    );
    assert!(
        tail.contains("coverage="),
        "and the coverage fact, so the model can tell an idle runtime from a blind index: {tail}"
    );
    // `in_scope`/`shown`/`omitted` are the coverage counts the model needs to read the fragment
    // correctly: a `Complete` with zero rows is a real "nothing is running", not an unread index.
    assert!(
        tail.contains("in_scope=") && tail.contains("shown=") && tail.contains("omitted="),
        "and the counts that make zero rows unambiguous: {tail}"
    );
    assert!(
        tail.contains("coverage=complete")
            || tail.contains("coverage=partial")
            || tail.contains("coverage=unavailable"),
        "the coverage state is one of the three declared values: {tail}"
    );

    // The fragment is in the DYNAMIC tail of that request, not the stable prefix: the stable
    // instruction text must be free of the marker.
    let stable = bodies[ordinal]["messages"]
        .as_array()
        .map(|messages| {
            messages
                .iter()
                .take(messages.len().saturating_sub(1))
                .filter(|m| m["role"] == "system" || m["role"] == "developer")
                .filter_map(|m| m["content"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    assert!(
        !stable.contains(FRAGMENT_MARKER),
        "the fragment must not appear in the stable prefix: {stable}"
    );

    // The real write really happened, so the run above was a genuine tool-driving step rather than a
    // run that answered immediately. Rather than guessing a root, this reads the **tool result the
    // fixture received back** - the model's own view of the call - and requires that the write
    // really succeeded. That is a fact about the run, not about a path this test assumed.
    let tool_results = bodies
        .iter()
        .flat_map(|body| {
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|message| message["role"] == "tool")
                .filter_map(|message| message["content"].as_str())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert!(
        !tool_results.is_empty(),
        "the Write scenario really produced a tool result, so a real tool step ran"
    );
    assert!(
        tool_results.iter().any(|result| result.contains("actual Agent Loop effect")
            || result.contains("\"succeeded\"")
            || result.contains("agent-result.txt")),
        "the tool result the real chain produced names its effect: {tool_results:?}"
    );
}

/// **R4** — a retry re-sends the frozen bytes instead of re-sampling the index.
///
/// # Why this is the same criterion as "one consistent picture per step"
///
/// The adapter is called once per transport attempt. If it sampled the index, a retry would send
/// different bytes than the attempt it is retrying, and the "same frozen context" the request
/// snapshot claims to describe would be false. This test drives the real retry-bearing provider
/// failure and compares the **captured** bodies of the two attempts: the activity fragment must be
/// byte-identical, because both came from one frozen value.
///
/// A test that rendered a frozen value twice in memory would prove the renderer is pure but not that
/// the production retry path reuses it; this reads both attempts off the wire.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_retry_resends_the_same_frozen_activity_bytes() {
    let host = crate::r03_tests::Fixture::new();
    // A real HTTP 503: the endpoint is reachable, the request was understood, and the Provider
    // refused to serve it - so the transport's own retry rule decides whether and how to re-send.
    let provider = Provider::scenario(Scenario::ProviderUnavailable).await;
    let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let _ = waiting::wait(&runtime, &request, |p| p.turn.state.terminal()).await;

    let bodies = captured(&provider);
    assert!(
        bodies.len() >= 2,
        "the real transport really re-sent the request: {} captured bodies",
        bodies.len()
    );

    // Compare the fragments of every captured attempt. A re-sampling implementation would drift
    // (a new `sampled_at`, a new `revision`), which is what this rejects.
    let fragments = bodies
        .iter()
        .map(dynamic_tail)
        .filter(|tail| tail.contains(FRAGMENT_MARKER))
        .collect::<Vec<_>>();
    assert!(
        !fragments.is_empty(),
        "at least one captured attempt must carry an activity fragment for this to measure anything"
    );
    assert!(
        fragments.windows(2).all(|pair| pair[0] == pair[1]),
        "every retry of one attempt must send byte-identical frozen fragment bytes; a difference \
         means the index was re-sampled: {}",
        fragments
            .iter()
            .map(|fragment| fragment.lines().next().unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
            .join(" || ")
    );
}

/// **R4** — each adapter that the fixture can serve really carries the fragment, captured per
/// adapter.
///
/// # Why this is separate from the single-adapter capture
///
/// C's 9/9 compiled one hardcoded body and asserted all three adapters placed *that* body correctly.
/// That is compile-layer evidence: it proves the adapters handle a body they are handed, and nothing
/// about whether Core produces one on each real path. This drives the **real** Agent per adapter
/// against the local fixture and reads each adapter's own captured request. A per-adapter assertion is
/// the only form that can fail for the adapter it names.
///
/// # Why the scenario differs per adapter, and what is NOT covered
///
/// The fixture's generic scenarios answer from a **chat-shaped** request (`body["messages"]`), so they
/// serve Chat Completions. The Responses wire is served by its own scenario
/// (`ResponsesWriteThenIllegalToolName`), which also drives a real `fs.write`. **The Anthropic
/// Messages wire has no fixture scenario in this tree at all** - no existing test drives it on the
/// real chain - so it is listed as *not obtained* rather than asserted through a substitute. That is
/// the honest position: an adapter whose real chain cannot be driven here has no captured bytes, and
/// the hardcoded body cannot stand in for them.
///
/// The dynamic tail lives in a different field per adapter, which is why `dynamic_tail` reads each
/// one's real shape instead of assuming a single one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_connected_adapter_really_carries_the_fragment() {
    use aura_model_domain::ModelApiFormat;

    for (label, format, scenario) in [
        ("openai_chat_completions", ModelApiFormat::OpenAiChatCompletions, Scenario::Write),
        (
            "openai_responses",
            ModelApiFormat::OpenAiResponses,
            Scenario::ResponsesWriteThenIllegalToolName,
        ),
    ] {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(scenario).await;
        let runtime = host.start().await;
        let model = super::profile_for(&runtime, &provider.url, format).await;
        let root = host.root.path().join(format!("adapter-{label}-workspace"));
        std::fs::create_dir(&root).expect("workspace root");
        let created = crate::r03_tests::success(
            &runtime,
            "workspace.create",
            serde_json::json!({"selected_root":root,"display_name":format!("adapter {label}")}),
        )
        .await;
        let workspace: aura_workspace_domain::WorkspaceSummary =
            serde_json::from_value(created["workspace"].clone()).expect("workspace summary");
        let session = super::create_session(
            &runtime,
            workspace.workspace_id,
            aura_security_contract::PermissionMode::WorkspaceReadWrite,
        )
        .await;
        let started = super::waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
        let request = AgentRunRequest {
            session_id: started.turn.session_id,
            agent_run_id: started.run.agent_run_id,
        };
        let _ = super::waiting::wait(&runtime, &request, |p| p.turn.state.terminal()).await;

        let bodies = captured(&provider);
        assert!(
            !bodies.is_empty(),
            "{label}: the real chain really asked the Provider, so there is a capture to read"
        );
        let carrier = bodies
            .iter()
            .map(dynamic_tail)
            .find(|tail| tail.contains(FRAGMENT_MARKER))
            .unwrap_or_else(|| {
                panic!(
                    "{label}: a real request must carry the activity fragment in its dynamic tail; \
                     the fixture captured {} bodies and none carried one",
                    bodies.len()
                )
            });
        assert!(
            carrier.contains("sampled_at=")
                && carrier.contains("revision=")
                && carrier.contains("coverage="),
            "{label}: the fragment carries the frozen snapshot's header: {carrier}"
        );
        println!("R4 adapter {label}: fragment carried, {} captured bodies", bodies.len());
    }
}
