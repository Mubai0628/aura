//! Multiple real sessions share one Core, workspace and authority store.
use super::*;

impl Fixture {
    pub fn additional_session(&self, mode: PermissionMode, names: &[&str]) -> Self {
        let workspace = self
            .service()
            .persistence
            .workspace(self.turn.workspace_id)
            .expect("the additional session shares the existing real workspace");
        Self::create_in_workspace(
            Arc::clone(&self._host),
            Arc::clone(&self.runtime),
            self.root.clone(),
            workspace,
            mode,
            names,
            super::packages::Packages::None,
        )
    }

    pub(super) fn create_in_workspace(
        host: Arc<crate::r03_tests::Fixture>,
        runtime: Arc<crate::RuntimeState>,
        root: std::path::PathBuf,
        workspace: aura_workspace_domain::WorkspaceSummary,
        mode: PermissionMode,
        names: &[&str],
        packages: super::packages::Packages,
    ) -> Self {
        let session = SessionId::new();
        let service = services(&runtime);
        service
            .persistence
            .upsert_workspace_session(
                workspace.clone(),
                aura_workspace_domain::SessionSummary {
                    session_id: session,
                    workspace_id: workspace.workspace_id,
                    title: "Agent batch".into(),
                    state: "idle".into(),
                },
                CorrelationId::new(),
            )
            .expect("Agent test setup and persisted assertion");
        service
            .persistence
            .ensure_session_permission_state(
                session,
                workspace.workspace_id,
                mode,
                CorrelationId::new(),
            )
            .expect("Agent test setup and persisted assertion");
        let settings = ToolSettingsSnapshotV1::new(Revision::INITIAL, 50, 2, 30_000, 0)
            .expect("Agent test setup and persisted assertion");
        let base = aura_context_fixtures::p06_prepared_context_fixture()
            .expect("Agent test setup and persisted assertion");
        let at = UtcTimestamp::now();
        let user = OriginalUserMessage::new(
            EventId::new(),
            session,
            "Read and verify the requested workspace files".into(),
            UserMessageOrigin::UserInput,
            at,
        )
        .expect("Agent test setup and persisted assertion");
        let (turn, step) = service
            .broker
            .lock()
            .expect("Agent test setup and persisted assertion")
            .database
            .unit_of_work(|tx| {
                if let Some(existing) = workspace::get(tx, workspace.workspace_id)? {
                    // Catalog updates can advance revision/freshness after a real Shell effect.
                    // The additional session must still use the same verified authority.
                    assert_eq!(existing.workspace_id, workspace.workspace_id);
                    assert_eq!(existing.canonical_root, workspace.canonical_root);
                    assert_eq!(existing.root_identity, workspace.root_identity);
                    assert_eq!(existing.authorization_epoch, workspace.authorization_epoch);
                    assert_eq!(existing.state, workspace.state);
                } else {
                    workspace::insert(tx, &workspace)?;
                }
                workspace::insert_session(
                    tx,
                    &workspace::SessionMetadata {
                        id: session,
                        workspace_id: workspace.workspace_id,
                        title: "Agent".into(),
                        created_at: at,
                        updated_at: at,
                    },
                )?;
                context::insert_bundle(tx, session, &base.prompt_bundle_snapshot)?;
                let turn = lifecycle::start(
                    tx,
                    &user,
                    AgentTaskMode::Normal,
                    None,
                    base.prompt_bundle_snapshot.id,
                )?;
                let step = lifecycle::prepare_step(
                    tx,
                    turn.agent_run_id,
                    AgentContinuation::Initial { user_event_id: user.event_id },
                    at,
                )?;
                Ok::<_, aura_persistence_core::PersistenceCoreError>((turn, step.model_step_id))
            })
            .expect("Agent test setup and persisted assertion");
        let release = aura_tool_registry::agent_release::release()
            .expect("Agent test setup and persisted assertion");
        // The production derivation of every installed package tool, loaded for
        // this session the way `tools.load` loads a discoverable tool.
        let installed = if packages != super::packages::Packages::None {
            crate::agent_execution::package_tools::installed_tools(
                service.broker.lock().expect("package facts").database.connection(),
            )
            .expect("installed package tools derive")
        } else {
            vec![]
        };
        // The production derivation of every usable MCP server's tools.
        let mcp_tools = if packages != super::packages::Packages::None {
            crate::agent_execution::mcp_tools::usable_tools(
                &service.mcp,
                service.broker.lock().expect("mcp facts").database.connection(),
            )
        } else {
            vec![]
        };
        let mut candidates = release.candidates();
        candidates.extend(installed.iter().map(|tool| tool.candidate()));
        candidates.extend(mcp_tools.iter().map(|tool| tool.candidate()));
        let loads = installed
            .iter()
            .map(|tool| &tool.definition)
            .chain(mcp_tools.iter().map(|tool| &tool.definition))
            .filter(|_| packages == super::packages::Packages::Loaded)
            .map(|definition| aura_tool_domain::ToolEphemeralLoadRecordV1 {
                session_id: session,
                turn_task_id: turn.turn_task_id,
                tool_id: definition.tool_id,
                tool_version: definition.tool_version.clone(),
                entity_revision: Revision::INITIAL,
                loaded_at: at,
            })
            .collect::<Vec<_>>();
        let registry = build_parent_registry_snapshot(ParentRegistryBuildInputV1 {
            registry_revision: Revision::INITIAL,
            scope: ToolRegistryScopeV1::Session {
                workspace_id: workspace.workspace_id,
                session_id: session,
                turn_task_id: turn.turn_task_id,
                agent_run_id: turn.agent_run_id,
            },
            settings: settings.clone(),
            mode_permission_context_hash: aura_domain_common::jcs::canonical_sha256(&mode)
                .expect("Agent test setup and persisted assertion"),
            core_exposed_tool_ids: names
                .iter()
                .map(|name| {
                    release
                        .entry(name)
                        .expect("Agent test setup and persisted assertion")
                        .definition
                        .tool_id
                })
                .collect(),
            candidates,
            workspace_pins: vec![],
            session_pins: vec![],
            ephemeral_loads: loads,
            created_at: at,
        })
        .expect("Agent test setup and persisted assertion")
        .snapshot;
        let mut named = crate::agent_execution::package_tools::named_by(&installed, &registry);
        named.extend(crate::agent_execution::mcp_tools::named_by(&mcp_tools, &registry));
        service
            .broker
            .lock()
            .expect("store package definitions with the snapshot")
            .database
            .unit_of_work(|tx| crate::agent_execution::package_tools::store(tx, &named))
            .expect("package definitions stored with the snapshot");
        steps::install_step(service, &turn, step, &registry, base.prompt_bundle_snapshot.id, true);
        Self {
            _host: host,
            runtime,
            root,
            turn,
            step,
            settings,
            bundle: base.prompt_bundle_snapshot.id,
        }
    }
}
