#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::AgentTaskMode;
use aura_domain_common::*;
use aura_persistence_v2::{V2Store, tool::session_tools};
use aura_tool_contract::SessionDynamicTools;

#[test]
fn dynamic_membership_is_empty_isolated_cas_and_persistent() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("store");
    let mut store = V2Store::create(&path).expect("store");
    let a = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let other = SessionId::new();
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::workspace::insert_session(
                tx,
                &aura_persistence_v2::workspace::SessionMetadata {
                    id: other,
                    workspace_id: a.turn.workspace_id,
                    title: "Other".into(),
                    created_at: a.at,
                    updated_at: a.at,
                },
            )
        })
        .expect("second session");
    assert!(
        session_tools::load(store.database.connection(), a.turn.session_id)
            .expect("empty")
            .is_none()
    );
    let state = SessionDynamicTools {
        session_id: a.turn.session_id,
        revision: Revision::INITIAL,
        tool_ids: vec![ToolId::new()],
    };
    store.database.unit_of_work(|tx| session_tools::save(tx, None, &state, a.at)).expect("save");
    assert!(
        session_tools::load(store.database.connection(), other).expect("other session").is_none()
    );
    assert!(store.database.unit_of_work(|tx| session_tools::save(tx, None, &state, a.at)).is_err());
    drop(store);
    let mut reopened = V2Store::open(&path).expect("reopen");
    assert_eq!(
        session_tools::load(reopened.database.connection(), a.turn.session_id).expect("read"),
        Some(state.clone())
    );
    let next =
        SessionDynamicTools { tool_ids: vec![], revision: Revision::new(2), ..state.clone() };
    reopened
        .database
        .unit_of_work(|tx| session_tools::save(tx, Some(&state), &next, a.at))
        .expect("explicit unload");
    assert_eq!(
        session_tools::load(reopened.database.connection(), a.turn.session_id).expect("read"),
        Some(next)
    );
}

#[test]
fn issued_step_settings_and_configuration_command_identity_survive_restart() {
    use aura_persistence_v2::{
        agent::application,
        tool::configuration::{self, ConfigurationCommand},
    };
    use aura_tool_contract::{
        AgentToolSettingsUpdate, AgentToolSettingsView, DEFAULT_MAX_CONCURRENT_TOOL_CALLS,
        DEFAULT_MAX_TOOL_RETRIES, DEFAULT_TOOL_TIMEOUT_MS, ToolSettingsSnapshotV1,
    };
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("store");
    let mut store = V2Store::create(&path).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let settings = ToolSettingsSnapshotV1::default();
    store
        .database
        .unit_of_work(|tx| {
            application::freeze_step_tool_settings(tx, f.step.model_step_id, &settings, f.at)
        })
        .expect("freeze");
    let changed =
        ToolSettingsSnapshotV1::new(Revision::new(settings.revision.value() + 1), 1, 2, 30_000, 0)
            .expect("changed");
    assert!(
        store
            .database
            .unit_of_work(|tx| application::freeze_step_tool_settings(
                tx,
                f.step.model_step_id,
                &changed,
                f.at
            ))
            .is_err()
    );
    let command = CommandInvocationId::new();
    let intent = ConfigurationCommand {
        input: AgentToolSettingsUpdate {
            expected_revision: Revision::INITIAL,
            fixed_tool_ids: vec![ToolId::new()],
            max_dynamic_tool_schemas: 1,
            max_concurrent_tool_calls: DEFAULT_MAX_CONCURRENT_TOOL_CALLS,
            default_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
            max_retries: DEFAULT_MAX_TOOL_RETRIES,
            use_recommended_fixed_tool_ids: false,
            network_proxy: None,
            web_service: None,
        },
        result: None,
    };
    store
        .database
        .unit_of_work(|tx| configuration::save(tx, command, &intent, f.at))
        .expect("intent");
    let mut forged = intent.clone();
    forged.input.fixed_tool_ids.clear();
    assert!(
        store.database.unit_of_work(|tx| configuration::save(tx, command, &forged, f.at)).is_err()
    );
    drop(store);
    let mut reopened = V2Store::open(&path).expect("reopen");
    assert_eq!(
        application::step_tool_settings(reopened.database.connection(), f.step.model_step_id)
            .expect("frozen"),
        settings
    );
    assert_eq!(
        configuration::load(reopened.database.connection(), command).expect("intent survives"),
        Some(intent.clone())
    );
    let done = ConfigurationCommand {
        result: Some(AgentToolSettingsView {
            revision: Revision::new(2),
            max_dynamic_tool_schemas: 1,
            max_concurrent_tool_calls: DEFAULT_MAX_CONCURRENT_TOOL_CALLS,
            default_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
            max_retries: DEFAULT_MAX_TOOL_RETRIES,
            fixed_tool_ids: intent.input.fixed_tool_ids.clone(),
            tools: vec![],
            network_proxy: None,
            network_proxy_diagnostic: None,
            network_proxy_draft: None,
            network_proxy_recovery: None,
            web_service: Default::default(),
            web_service_connections: vec![],
        }),
        ..intent
    };
    reopened
        .database
        .unit_of_work(|tx| configuration::save(tx, command, &done, f.at))
        .expect("receipt");
    reopened
        .database
        .unit_of_work(|tx| configuration::save(tx, command, &done, f.at))
        .expect("idempotent receipt");
    assert!(
        reopened
            .database
            .unit_of_work(|tx| configuration::save(tx, command, &forged, f.at))
            .is_err()
    );
}
