use super::*;

pub(super) struct Fixture {
    pub(super) _root: tempfile::TempDir,
    pub(super) store: V2Store,
    pub(super) session: SessionId,
    pub(super) workspace: WorkspaceId,
    pub(super) snapshot: PermissionSnapshot,
    pub(super) runtime: RuntimeInstanceId,
    pub(super) principal: PrincipalId,
    pub(super) run: AgentRunId,
    pub(super) turn: TurnTaskId,
    /// The clock this case reads. Advanced deliberately, never slept through.
    pub(super) at: UtcTimestamp,
}

impl Fixture {
    pub(super) fn new(mode: PermissionMode) -> Self {
        let root = tempfile::tempdir().expect("isolated store");
        let directory = root.path().join("v2");
        let mut store = V2Store::create(&directory).expect("store");
        let workspace = WorkspaceId::new();
        let session = SessionId::new();
        let at = UtcTimestamp::now();
        store
            .database
            .unit_of_work(|tx| {
                workspace::insert(
                    tx,
                    &aura_workspace_domain::WorkspaceSummary {
                        workspace_id: workspace,
                        display_name: "native session".into(),
                        selected_root: "C:\\native-session".into(),
                        canonical_root: "C:\\native-session".into(),
                        root_identity: None,
                        state: aura_workspace_domain::WorkspaceState::Online,
                        revision: Revision::INITIAL,
                        catalog_freshness: aura_workspace_domain::CatalogFreshness::Unavailable,
                        authorization_epoch: Revision::INITIAL,
                    },
                )?;
                workspace::insert_session(
                    tx,
                    &workspace::SessionMetadata {
                        id: session,
                        workspace_id: workspace,
                        title: "native session".into(),
                        created_at: at,
                        updated_at: at,
                    },
                )
            })
            .expect("workspace");
        let snapshot = PermissionSnapshot {
            id: PermissionSnapshotId::new(),
            session_id: session,
            workspace_id: workspace,
            mode,
            session_permission_revision: 1,
            active_grant_ids: vec![],
            created_at: at,
        };
        store
            .database
            .unit_of_work(|tx| security::insert_snapshot(tx, &snapshot))
            .expect("snapshot");
        Self {
            _root: root,
            store,
            session,
            workspace,
            snapshot,
            runtime: RuntimeInstanceId::new(),
            principal: PrincipalId::new(),
            run: AgentRunId::new(),
            turn: TurnTaskId::new(),
            at,
        }
    }

    pub(super) fn advance_minutes(&mut self, minutes: i64) {
        self.at = UtcTimestamp::from_value(self.at.value() + time::Duration::minutes(minutes));
    }

    /// A fresh permission snapshot for one call.
    ///
    /// A real call carries the snapshot that was current when it was admitted, and
    /// the reservation reloads exactly that snapshot from the Store. A case that
    /// reused one snapshot id for every call would test a shape the product never
    /// produces, so each new call gets its own stored snapshot at the same
    /// permission revision.
    pub(super) fn fresh_snapshot(&mut self) -> PermissionSnapshot {
        let snapshot = PermissionSnapshot {
            id: PermissionSnapshotId::new(),
            session_id: self.session,
            workspace_id: self.workspace,
            mode: self.snapshot.mode,
            session_permission_revision: self.snapshot.session_permission_revision,
            active_grant_ids: vec![],
            created_at: self.at,
        };
        self.store
            .database
            .unit_of_work(|tx| security::insert_snapshot(tx, &snapshot))
            .expect("snapshot");
        snapshot
    }

    /// A Shell Operation with its own script, correlation and snapshot, so each call
    /// is a genuinely new Operation fingerprint bound to a real snapshot.
    pub(super) fn operation(&mut self, shell: &str, script: &str) -> SecuredOperation {
        let snapshot = self.fresh_snapshot();
        self.operation_in(
            shell,
            script,
            &snapshot,
            self.principal,
            SecuritySourceRef::BuiltInTool {
                identity_hash: CanonicalSha256HashV1::sha256(b"builtin-shell"),
            },
        )
    }

    pub(super) fn operation_in(
        &self,
        shell: &str,
        script: &str,
        snapshot: &PermissionSnapshot,
        principal: PrincipalId,
        source: SecuritySourceRef,
    ) -> SecuredOperation {
        let mut identity =
            InvocationIdentity::runtime_user(self.workspace, self.session, CorrelationId::new());
        identity.principal_id = principal;
        identity.agent_run_id = Some(self.run);
        identity.turn_task_id = Some(self.turn);
        identity.permission_snapshot_id = Some(snapshot.id);
        SecuredOperation::new(
            aura_domain_common::SecurityOperationId::new(),
            aura_execution_contract::OperationContent {
                compatibility: Default::default(),
                network: OperationNetwork::None,
                identity,
                source,
                source_revision_fingerprint: None,
                capabilities: vec![ResolvedCapability::ProcessExecute],
                // A Shell Operation always names the scope it runs in; the workspace
                // root is that scope, and an Operation with no resource at all is
                // rejected before any of this matters.
                resources: vec![ResourceDescriptor::WorkspaceRoot { workspace_id: self.workspace }],
                risks: vec![],
                credentials: vec![],
                purpose: None,
                action_kind: shell.into(),
                action_revision: Revision::INITIAL,
                action: serde_json::json!({ "script": script }),
            },
        )
        .expect("operation")
    }

    /// The Native backend's declared capabilities, so policy can assess a call at
    /// all. A descriptor missing them is refused as `BackendUnavailable` before the
    /// consent is consulted, which would be a different assertion than the one each
    /// case below is making.
    pub(super) fn native_backend(&self) -> BackendDescriptor {
        BackendDescriptor {
            id: BackendId(aura_backend_windows_user::NATIVE_BACKEND_ID.into()),
            revision: Revision::INITIAL,
            runtime_instance_id: self.runtime,
            class: BackendClass::Native,
            compatibility: [ExecutionCompatibility::ProcessTreeCancellation].into(),
            capabilities: [
                ResolvedCapability::ProcessExecute,
                ResolvedCapability::WorkspaceFileRead,
                ResolvedCapability::WorkspaceFileWrite,
                ResolvedCapability::WorkspaceFileCreate,
                ResolvedCapability::WorkspaceFileDelete,
                ResolvedCapability::WorkspaceFileMove,
            ]
            .into(),
            guarantees: [
                EnforcementGuarantee::ChildProcessControl,
                EnforcementGuarantee::BoundedOutput,
            ]
            .into(),
            known_limitations: Default::default(),
            readiness: BackendReadiness::Ready,
            protocol_fingerprint: CanonicalSha256HashV1::sha256(b"native-protocol"),
        }
    }

    /// One user approval of `operation`, answered with `choice`, at the current
    /// clock with a confirmation window of `window_minutes`.
    pub(super) fn approve_at(
        &mut self,
        operation: &SecuredOperation,
        choice: ApprovalDecisionKind,
        window_minutes: i64,
    ) -> Vec<AuthorizationGrant> {
        self.approve_at_runtime(operation, choice, window_minutes, self.runtime)
    }

    /// One approval issued by a named Core runtime instance, so a case can act as a
    /// later Core run looking at an earlier run's consent.
    pub(super) fn approve_at_runtime(
        &mut self,
        operation: &SecuredOperation,
        choice: ApprovalDecisionKind,
        window_minutes: i64,
        runtime: RuntimeInstanceId,
    ) -> Vec<AuthorizationGrant> {
        let requested_at = self.at;
        let expires_at = UtcTimestamp::from_value(
            requested_at.value() + time::Duration::minutes(window_minutes),
        );
        let approval = AuthorizationApproval {
            approval_id: aura_domain_common::ApprovalRequestId::new(),
            revision: Revision::INITIAL,
            scope: AuthorizationScope {
                operation_fingerprint: operation.fingerprint.clone(),
                profile: SecurityProfile::FullAccess,
                backend_requirement: BackendRequirement {
                    allowed_classes: [BackendClass::Native].into(),
                    guarantees: Default::default(),
                },
                lifetime: AuthorizationLifetime::Once { operation_id: operation.operation_id },
                consent: None,
                network_consent: None,
                capability_limit: None,
                memory_consent: None,
                mcp_consent: None,
                file_consent: None,
            },
            allowed_decisions: vec![
                ApprovalDecisionKind::AllowOnce,
                ApprovalDecisionKind::AllowSession,
                ApprovalDecisionKind::Deny,
            ],
            state: ApprovalState::Pending,
            requested_at,
            expires_at,
        };
        let at = self.at;
        let revision = self.snapshot.session_permission_revision;
        self.store
            .database
            .unit_of_work(|tx| {
                security::insert_approval(tx, &approval)?;
                security_approval::resolve(
                    tx,
                    approval.approval_id,
                    Revision::INITIAL,
                    operation,
                    choice,
                    // The consent only ever comes from an answer about the Native
                    // execution path, so the case supplies that backend class.
                    BackendClass::Native,
                    revision,
                    runtime,
                    at,
                )
            })
            .expect("approval resolution")
            .into_iter()
            .collect()
    }

    /// One approval with the standard ten-minute confirmation window.
    pub(super) fn approve(
        &mut self,
        operation: &SecuredOperation,
        choice: ApprovalDecisionKind,
    ) -> Vec<AuthorizationGrant> {
        self.approve_at(operation, choice, 10)
    }

    /// One approval issued by a named runtime instance.
    pub(super) fn approve_with_runtime(
        &mut self,
        operation: &SecuredOperation,
        choice: ApprovalDecisionKind,
        runtime: RuntimeInstanceId,
    ) -> Vec<AuthorizationGrant> {
        self.approve_at_runtime(operation, choice, 10, runtime)
    }

    /// A decision derived from the consent a new call is allowed to reuse, at the
    /// current clock. This is the shape the dispatch path builds: the live Grants for
    /// the call, the one that covers it, and a decision whose scope names the
    /// Operation being dispatched.
    pub(super) fn derive(
        &self,
        operation: &SecuredOperation,
    ) -> Option<(AuthorizationGrant, AuthorizationDecision)> {
        let grants = security::grants_for_call(
            self.store.database.connection(),
            &operation.fingerprint,
            Some(self.session),
        )
        .expect("grants");
        let now = self.at;
        let expires = UtcTimestamp::from_value(now.value() + time::Duration::minutes(5));
        grants.into_iter().filter(|grant| grant.scope.consent.is_some()).find_map(|grant| {
            AuthorizationDecision::from_grant_for_operation(
                &grant,
                &operation.fingerprint,
                now,
                expires,
            )
            .map(|decision| (grant, decision))
        })
    }

    /// The owning transaction that reserves the Attempt, which is where a consent is
    /// re-checked against the live permission revision.
    ///
    /// The path is the production one: load the Grants the call may use, run the
    /// Final Dispatch Guard against them, then let `execution::prepare` reserve the
    /// authority and the Attempt inside the same transaction.
    pub(super) fn reserve(
        &mut self,
        operation: &SecuredOperation,
        backend: &BackendDescriptor,
        decision: &AuthorizationDecision,
        live_permission_revision: u64,
    ) -> aura_persistence_core::StoreResult<ExecutionAttempt> {
        let now = self.at;
        let plan = ExecutionPlan::new(PreparedExecution {
            operation_fingerprint: operation.fingerprint.clone(),
            backend: backend.clone(),
            preparation: serde_json::json!({ "fixture": "native-session" }),
        })
        .expect("plan");
        let request = DispatchRequest {
            operation: operation.clone(),
            binding: DispatchBinding {
                attempt: AttemptRef {
                    operation_id: operation.operation_id,
                    ordinal: std::num::NonZeroU32::MIN,
                },
                operation_fingerprint: operation.fingerprint.clone(),
                authorization_fingerprint: decision.fingerprint().expect("fingerprint"),
                plan_fingerprint: plan.fingerprint.clone(),
                cancellation_revision: Revision::INITIAL,
            },
            plan,
        };
        let grants = security::grants_for_call(
            self.store.database.connection(),
            &operation.fingerprint,
            Some(self.session),
        )?;
        // Policy reads the snapshot the call was admitted under, exactly as the
        // reservation does; using any other snapshot would test a shape the product
        // cannot produce.
        let policy_snapshot = security::snapshot(
            self.store.database.connection(),
            operation
                .content
                .identity
                .permission_snapshot_id
                .expect("the fixture binds a snapshot"),
        )?
        .expect("the fixture stores the snapshot it bound");
        let runtime = self.runtime;
        self.store.database.unit_of_work(|tx| {
            let checked = aura_permission_engine::dispatch_guard::revalidate(
                request,
                decision,
                operation,
                &aura_permission_engine::policy::PolicyInput {
                    network_caller: NetworkCallerKind::AuraTool,
                    operation,
                    snapshot: &policy_snapshot,
                    profile: SecurityProfile::FullAccess,
                    source_enabled: true,
                    trust: &TrustState::SystemTrusted,
                    source_fingerprint: None,
                    hard_boundaries_satisfied: true,
                    denied_for_turn: false,
                    grants: &grants,
                    verified_external_grants: &[],
                    verified_external_capabilities: &[],
                    now,
                    expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(5)),
                    core_runtime_instance_id: Some(runtime),
                },
                backend,
                &CancellationState { revision: Revision::INITIAL, requested: false },
            )
            .map_err(|_| aura_persistence_core::PersistenceCoreError::Conflict)?;
            // The owning transaction records the Operation before it reserves the
            // Attempt, exactly as `security_control::reserve_in_transaction` does.
            // `execution::prepare` reads that row back, so a reservation that skipped
            // it would fail for a reason that has nothing to do with the consent.
            let payload = serde_json::to_value(checked.request().operation.clone())?;
            if aura_persistence_core::operation::begin(
                tx,
                &aura_persistence_core::operation::OperationDraft {
                    id: operation.operation_id,
                    domain: "execution",
                    kind: &operation.content.action_kind,
                    request_key: &operation.operation_id.to_string(),
                    version: 1,
                    payload: &payload,
                    at: now,
                },
            )? != aura_persistence_core::operation::BeginOperation::Created
            {
                return Err(aura_persistence_core::PersistenceCoreError::Conflict);
            }
            aura_persistence_v2::execution::prepare(
                tx,
                &checked,
                decision,
                live_permission_revision,
                now,
            )
        })
    }

    /// The management projection's inputs, read the way the route reads them.
    pub(super) fn consent_view(
        &self,
        grant_id: PermissionGrantId,
    ) -> Option<NativeSessionConsentView> {
        security::grant(self.store.database.connection(), grant_id).expect("grant").and_then(
            |grant| {
                security::native_session_consent(self.store.database.connection(), grant_id)
                    .expect("consent")
                    .and_then(|consent| {
                        let approval_expires_at = security_approval::load(
                            self.store.database.connection(),
                            consent.approval_id,
                        )
                        .map(|approval| approval.expires_at)
                        .unwrap_or(grant.issued_at);
                        project(
                            &consent,
                            &grant,
                            self.workspace,
                            approval_expires_at,
                            LiveFacts {
                                runtime: self.runtime,
                                permission_revision: self.snapshot.session_permission_revision,
                            },
                            self.at,
                        )
                    })
            },
        )
    }

    /// The management projection of one stored row against one Grant.
    ///
    /// Used by the binding cases, where the question is whether a row and a Grant that
    /// disagree about an immutable field are still shown as one authorization.
    pub(super) fn consent_row_view(
        &self,
        consent: &security::NativeSessionConsent,
        grant: &AuthorizationGrant,
    ) -> Option<NativeSessionConsentView> {
        project(
            consent,
            grant,
            self.workspace,
            self.at,
            LiveFacts {
                runtime: self.runtime,
                permission_revision: self.snapshot.session_permission_revision,
            },
            self.at,
        )
    }

    /// The management projection with different live facts, for the invalidation
    /// cases.
    pub(super) fn consent_view_with(
        &self,
        grant_id: PermissionGrantId,
        facts: LiveFacts,
    ) -> Option<NativeSessionConsentView> {
        security::grant(self.store.database.connection(), grant_id).expect("grant").and_then(
            |grant| {
                security::native_session_consent(self.store.database.connection(), grant_id)
                    .expect("consent")
                    .and_then(|consent| {
                        project(&consent, &grant, self.workspace, grant.issued_at, facts, self.at)
                    })
            },
        )
    }
}

/// The live facts a consent's usability depends on, as Core supplies them.
#[derive(Clone, Copy)]
pub(super) struct LiveFacts {
    pub(super) runtime: RuntimeInstanceId,
    pub(super) permission_revision: u64,
}

/// Mirrors `aura_core::native_session_grants::view` without depending on Core.
///
/// The Core projection has its own unit tests; this keeps the store assertions able
/// to show that a Grant whose consent reference moved is refused rather than
/// projected as still usable, and that a withdrawn consent is projected as revoked.
pub(super) fn project(
    consent: &security::NativeSessionConsent,
    grant: &AuthorizationGrant,
    workspace_id: WorkspaceId,
    approval_expires_at: UtcTimestamp,
    live: LiveFacts,
    now: UtcTimestamp,
) -> Option<NativeSessionConsentView> {
    if !consent.matches_grant(grant) {
        return None;
    }
    // A Grant that ran out of time and a consent this Core run no longer honours are
    // both "not usable right now", but they do not read the same: a consent an earlier
    // Core run issued has been superseded by the restart, while one approved under
    // other permissions has expired. The dispatch path checks a stronger property than
    // the label, so the assertions below also distinguish them by the reservation.
    let availability = if grant.revoked_at.is_some() {
        NativeSessionConsentAvailability::Revoked
    } else if grant.consumed_at.is_some() {
        NativeSessionConsentAvailability::Consumed
    } else if grant.issued_at > now {
        NativeSessionConsentAvailability::NotYetValid
    } else if grant.expires_at.is_some_and(|expires| now >= expires) {
        NativeSessionConsentAvailability::Expired
    } else if consent.lifetime == aura_security_contract::ShellConsentLifetime::LegacyRuntime
        && consent.runtime_instance != live.runtime
    {
        NativeSessionConsentAvailability::Superseded
    } else if consent.lifetime == aura_security_contract::ShellConsentLifetime::LegacyRuntime
        && consent.permission_revision != live.permission_revision
    {
        NativeSessionConsentAvailability::Expired
    } else {
        NativeSessionConsentAvailability::InForce
    };
    Some(NativeSessionConsentView {
        grant_id: consent.grant_id,
        grant_revision: grant.revision,
        approval_id: consent.approval_id,
        scope: NativeSessionConsentScopeView {
            lifetime: consent.lifetime,
            session_id: consent.session_id,
            workspace_id,
            approved_shell: consent.approved.approved_shell,
            shells: consent.approved.shells.clone(),
            principal_id: consent.approved.principal_id,
            approved_permission_revision: consent.permission_revision,
            core_runtime_instance_id: consent.runtime_instance,
            approved_operation_fingerprint: consent.approved_operation_fingerprint.clone(),
        },
        availability,
        issued_at: grant.issued_at,
        approved_through: approval_expires_at,
        revoked_at: grant.revoked_at,
        backend_id: aura_backend_windows_user::NATIVE_BACKEND_ID.to_owned(),
    })
}
