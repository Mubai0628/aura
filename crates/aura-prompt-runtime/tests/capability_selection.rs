//! How every turn chooses the way to do a task: the way the user asked for first, and when they did
//! not say how, the way that costs least time and tokens.
use aura_context_domain::PromptActivationCondition;
use aura_prompt_runtime::ValidatedPromptBundle;

#[test]
fn every_turn_is_told_to_take_the_users_way_first_and_otherwise_the_cheapest() {
    let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
    let selection = bundle
        .modules
        .iter()
        .find(|module| module.module_id == "core.capability-selection")
        .expect("the capability selection module");
    assert_eq!(selection.activation, PromptActivationCondition::Always, "sent with every turn");
    let text = &selection.normalized_content;
    assert!(
        text.contains("When the user said how to do something"),
        "the user's way first: {text}"
    );
    assert!(
        text.contains("say why before taking another"),
        "a way of the user's that cannot do it is said, not silently replaced: {text}"
    );
    assert!(text.contains("costs least time and tokens"), "otherwise the cheapest: {text}");
}
