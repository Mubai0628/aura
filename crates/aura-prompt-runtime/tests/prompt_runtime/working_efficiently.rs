//! Every turn hears how to keep its work cheap and in the user's hands: fewer steps, the deciding
//! facts first, the user asked before a workaround of the model's own, a one-time setup offered
//! for repeated asks, and large material read by its parts. Suggestions only: the module forbids
//! nothing, and the user's own way still comes first.
use aura_context_domain::PromptActivationCondition;
use aura_prompt_runtime::ValidatedPromptBundle;

#[test]
fn every_turn_hears_how_to_work_efficiently_as_suggestions() {
    let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
    let module = bundle
        .modules
        .iter()
        .find(|module| module.module_id == "core.working-efficiently")
        .expect("the working efficiently module");
    assert_eq!(module.activation, PromptActivationCondition::Always, "sent with every turn");
    let text = &module.normalized_content;
    for suggestion in [
        "Every step sends the whole conversation to the model again",
        "take a command's output from the call that runs it",
        "check the facts it rests on",
        "Keep doing independent authorized work.",
        "Ask when a missing decision, access, or prerequisite blocks the work",
        "offer a one-time setup that ends the repetition",
        "read a large file in ranges",
        "The user's own way still comes first",
    ] {
        assert!(text.contains(suggestion), "{suggestion}: {text}");
    }
    let lower = text.to_lowercase();
    for limit in ["never", "must", "do not", "don't"] {
        assert!(!lower.contains(limit), "a suggestion, not a limit: {limit}: {text}");
    }
}
