use aura_config::PromptSettingsSnapshot;
use aura_context_domain::PromptAgentMode;
use aura_prompt_runtime::{PromptActivationInput, ValidatedPromptBundle, activate_prompt_sources};

#[test]
fn normal_considers_remaining_work_before_a_progress_update_or_final_answer() {
    let bundle = ValidatedPromptBundle::embedded().expect("bundled English prompts");
    let active = activate_prompt_sources(
        &bundle,
        &PromptSettingsSnapshot { revision: 1, custom_system_addendum: None },
        &PromptActivationInput {
            agent_mode: PromptAgentMode::Normal,
            loaded_capabilities: Default::default(),
            child_agent: false,
            recovery_kinds: Default::default(),
            compaction: false,
            maintenance_kind: None,
        },
    )
    .expect("active Normal prompts");
    let text = active.sources.iter().map(|s| s.content.as_str()).collect::<Vec<_>>().join("\n");
    let (before_update, after_update) = text.split_once("session.reply").expect("interim tool");
    assert!(
        before_update.contains("authorized work or verification remains")
            && before_update.contains("pending decision"),
        "evaluate the whole request and independent work before choosing a progress reply"
    );
    let (work, _) = after_update.split_once("final answer ends the turn").expect("natural ending");
    assert!(work.contains("next action in the same turn"), "a status update leads back to work");
    assert!(
        work.contains("all remaining work") && work.contains("user asks to stop"),
        "offer the final response after its conditions"
    );
    assert!(text.contains("ordinary question directly"), "a standalone answer can finish");
}
