//! A turn that can run commands hears that an interactive program, or a shell that keeps its state
//! between commands, suits an Agent terminal better than a one-off script, and where the terminal
//! tools are. A suggestion: the paragraph forbids nothing.
use aura_context_domain::PromptActivationCondition;
use aura_prompt_runtime::ValidatedPromptBundle;

#[test]
fn a_turn_that_runs_commands_hears_when_an_agent_terminal_suits_better() {
    let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
    let module = bundle
        .modules
        .iter()
        .find(|module| module.module_id == "capabilities.shell-terminal-process")
        .expect("the shell and terminal module");
    assert_ne!(module.activation, PromptActivationCondition::Always, "sent with its capability");
    let text = &module.normalized_content;
    let paragraph = text
        .split("\n\n")
        .find(|paragraph| paragraph.contains("an Agent terminal suits better"))
        .unwrap_or_else(|| panic!("the interactive terminal paragraph: {text}"));
    for part in [
        "an interactive program or a shell that keeps its state between commands",
        "an SSH or database session, a REPL or a login prompt",
        "open one with terminal.open and send input with terminal.write, which tools.search finds",
    ] {
        assert!(paragraph.contains(part), "{part}: {paragraph}");
    }
    let lower = paragraph.to_lowercase();
    for limit in ["never", "must", "do not", "don't", "only"] {
        assert!(!lower.contains(limit), "a suggestion, not a limit: {limit}: {paragraph}");
    }
}
