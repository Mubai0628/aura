use aura_context_domain::PromptActivationCondition;
use aura_prompt_runtime::ValidatedPromptBundle;

#[test]
fn clarification_policy_covers_planning_and_silent_users_during_execution() {
    let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
    let shared = bundle
        .modules
        .iter()
        .find(|module| module.module_id == "core.user-interaction")
        .expect("shared user interaction");
    assert_eq!(shared.activation, PromptActivationCondition::Always);
    let prose = &shared.normalized_content;
    for behavior in [
        "use query.user_question",
        "In Plan mode or whenever the user asks you to design a plan",
        "confirm material uncertainties",
        "Call query.user_question in the current turn",
        "leave unconfirmed parts open",
        "work that does not depend on the answer before opening",
        "continue the authorized work",
        "list all unanswered questions together",
    ] {
        assert!(prose.contains(behavior), "{behavior} must reach the model");
    }
    assert!(prose.is_ascii(), "the shared rule must be written entirely in English");
}
