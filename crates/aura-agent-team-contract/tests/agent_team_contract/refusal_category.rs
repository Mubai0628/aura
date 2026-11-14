//! Which refused team calls are recorded as the call's own mistake, which as a missing target, and
//! which stay the tool's failure.
use aura_agent_team_contract::refusal_category;
use aura_domain_common::ErrorCategory;

#[test]
fn a_refusal_of_the_calls_own_input_is_a_request_the_model_can_correct() {
    for code in [
        "agents.input_invalid",
        "agents.name_invalid",
        "agents.send_invalid",
        "agents.send_names_conversation",
        "agents.task_incomplete",
        "agents.task_names_conversation",
        "agents.task_too_long",
        "agents.title_invalid",
        "agent.report_invalid",
        "agent.report_final_answer",
    ] {
        assert_eq!(refusal_category(code), Some(ErrorCategory::Validation), "{code}");
    }
}

#[test]
fn a_sub_agent_the_conversation_does_not_have_is_a_missing_target() {
    assert_eq!(refusal_category("agents.unknown_agent"), Some(ErrorCategory::NotFound));
}

#[test]
fn a_refusal_about_the_teams_state_stays_the_tools_failure() {
    for code in [
        "agents.no_slot",
        "agents.budget_used_up",
        "agents.stopped",
        "agents.still_running",
        "agents.not_git",
        "agents.not_for_subagents",
        "agents.unavailable",
        "git.outcome_unknown",
    ] {
        assert_eq!(refusal_category(code), None, "{code}");
    }
}
