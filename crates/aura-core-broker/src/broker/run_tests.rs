//! A command refused because its prepared facts changed: retried as a new Operation by a route
//! that opts in, refused as before by one that does not, and refused with a retryable answer once
//! the facts never settle.
use super::completion::PostExecutionOwners;
use super::journal::BrokerJournal;
use super::run::{MainChannel, OnChangedFacts, RunContext, run};
use super::{OwnedEvidence, OwnedInspect, descriptor};
use aura_core_base::command_claims::CommandOutcomeLedger;
use aura_core_base::command_settlement::CommandSettlements;
use aura_core_base::sync_boundary::SyncBoundary;
use aura_domain_common::{
    CommandInvocationId, CorrelationId, IpcChannelId, PrincipalId, Revision, RuntimeInstanceId,
    SecurityOperationId,
};
use aura_execution_contract::*;
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::*;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Fixture {
    _root: tempfile::TempDir,
    journal: Arc<BrokerJournal>,
    boundary: SyncBoundary,
    post_execution: PostExecutionOwners,
    settlements: CommandSettlements,
    runtime: RuntimeInstanceId,
    channel: MainChannel,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("isolated root");
        let journal = Arc::new(BrokerJournal::open(&root.path().join("journal")).expect("journal"));
        let runtime = RuntimeInstanceId::new();
        Self {
            _root: root,
            journal,
            boundary: SyncBoundary::open(),
            post_execution: PostExecutionOwners::new(runtime),
            settlements: CommandSettlements::new(
                runtime,
                Arc::new(Mutex::new(CommandOutcomeLedger::default())),
            ),
            runtime,
            channel: MainChannel {
                core_runtime_instance_id: runtime,
                authenticated_main_runtime_instance_id: RuntimeInstanceId::new(),
                authenticated_channel_id: IpcChannelId::new(),
            },
        }
    }

    /// Runs one command whose handler counts its calls.
    async fn run(
        &self,
        key: &str,
        on_changed: OnChangedFacts,
        inspect: OwnedInspect,
        calls: &Arc<AtomicUsize>,
    ) -> Result<Value, IpcErrorEnvelope> {
        let channel = self.channel;
        let current = move || Ok(channel);
        let context = RunContext {
            journal: &self.journal,
            boundary: &self.boundary,
            post_execution: &self.post_execution,
            settlements: &self.settlements,
            channel: &current,
            descriptor: descriptor(self.runtime),
        };
        let calls = Arc::clone(calls);
        run(
            context,
            operation(),
            key.into(),
            Some(CommandInvocationId::new()),
            on_changed,
            inspect,
            no_effect(),
            move |_| async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"done": true}))
            },
        )
        .await
    }

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> Vec<T> {
        let store = self.journal.lock().expect("store");
        let mut statement = store.database.connection().prepare(sql).expect("query");
        statement
            .query_map([], |row| row.get::<_, T>(0))
            .expect("rows")
            .collect::<Result<_, _>>()
            .expect("values")
    }
}

/// One recorded result as "outcome side-effect first-warning".
const RESULT: &str = "json_extract(payload,'$.outcome')||' '||json_extract(payload,'$.side_effect')||' '||coalesce(json_extract(payload,'$.warnings[0]'),'-')";

fn operation() -> SecuredOperation {
    SecuredOperation::new(
        SecurityOperationId::new(),
        OperationContent {
            compatibility: Default::default(),
            network: OperationNetwork::None,
            identity: InvocationIdentity {
                principal_id: PrincipalId::new(),
                actor: InvocationActor::User,
                workspace_id: None,
                session_id: None,
                turn_task_id: None,
                agent_run_id: None,
                parent_agent_run_id: None,
                correlation_id: CorrelationId::new(),
                permission_snapshot_id: None,
            },
            source: SecuritySourceRef::UserInterface,
            source_revision_fingerprint: None,
            capabilities: vec![ResolvedCapability::ConfigurationRead],
            resources: vec![ResourceDescriptor::ExternalService {
                service_id: "fixture".into(),
                object_ref: None,
            }],
            risks: vec![],
            credentials: vec![],
            purpose: None,
            action_kind: "fixture.read".into(),
            action_revision: Revision::INITIAL,
            action: json!({}),
        },
    )
    .expect("fixture operation")
}

fn no_effect() -> OwnedEvidence<Value> {
    Arc::new(|request: &DispatchRequest, _: &Result<Value, IpcErrorEnvelope>| {
        Ok(BrokerEvidence {
            action_kind: request.operation.content.action_kind.clone(),
            request_fingerprint: request.operation.fingerprint.0.clone(),
            before: request.plan.prepared.preparation.clone(),
            after: Value::Null,
            domain_journal: Value::Null,
            side_effect: SideEffectStateV1::NoEffect,
        })
    })
}

/// Facts that read `revision` 0 once and then 1: the first attempt is prepared on 0 and finds 1
/// when it is about to start.
fn changed_once(reads: &Arc<AtomicUsize>) -> OwnedInspect {
    let reads = Arc::clone(reads);
    Arc::new(move || {
        let read = reads.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"revision": usize::from(read > 0)}))
    })
}

/// Facts that read once and then cannot be read: the check before the start fails.
fn unreadable_after_preparing(reads: &Arc<AtomicUsize>) -> OwnedInspect {
    let reads = Arc::clone(reads);
    Arc::new(move || match reads.fetch_add(1, Ordering::SeqCst) {
        0 => Ok(json!({"revision": 0})),
        _ => Err(anyhow::anyhow!("the facts cannot be read")),
    })
}

/// Facts that differ on every read.
fn never_settling(reads: &Arc<AtomicUsize>) -> OwnedInspect {
    let reads = Arc::clone(reads);
    Arc::new(move || Ok(json!({"revision": reads.fetch_add(1, Ordering::SeqCst)})))
}

#[tokio::test]
async fn facts_that_changed_once_are_prepared_again_and_the_handler_runs_once() {
    let fixture = Fixture::new();
    let (reads, calls) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let result =
        fixture.run("changed-once", OnChangedFacts::Retry, changed_once(&reads), &calls).await;
    assert_eq!(result.expect("the second attempt runs"), json!({"done": true}));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "the handler runs exactly once");
    assert_eq!(
        fixture.query::<String>(
            "SELECT request_key FROM operation_idempotency WHERE domain='core_broker' ORDER BY request_key"
        ),
        ["changed-once", "changed-once#retry-1"],
        "the retry is a new Operation with its own ledger key"
    );
    assert_eq!(
        fixture
            .query::<String>(&format!("SELECT {RESULT} FROM execution_results ORDER BY outcome")),
        ["failed no_effect execution.not_dispatched", "succeeded no_effect -"],
        "the refused attempt is recorded as not run, the retry as run"
    );
    assert_eq!(fixture.settlements.pending(), 0, "the invocation's claim ended with its run");
}

#[tokio::test]
async fn a_route_that_does_not_opt_in_is_refused_as_before() {
    let fixture = Fixture::new();
    let (reads, calls) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let refused = fixture
        .run("refused", OnChangedFacts::Refuse, changed_once(&reads), &calls)
        .await
        .expect_err("a changed fact refuses the command");
    assert_eq!(refused.code.as_str(), "broker.dispatch_unavailable");
    assert!(!refused.retryable);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(reads.load(Ordering::SeqCst), 2, "one preparation and one check, no retry");
    assert_eq!(
        fixture.query::<String>("SELECT request_key FROM operation_idempotency"),
        ["refused"]
    );
    assert_eq!(fixture.settlements.pending(), 0);
}

#[tokio::test]
async fn a_check_that_fails_is_not_retried_by_a_route_that_opts_in() {
    let fixture = Fixture::new();
    let (reads, calls) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let refused = fixture
        .run("unreadable", OnChangedFacts::Retry, unreadable_after_preparing(&reads), &calls)
        .await
        .expect_err("a check that fails refuses the command");
    assert_ne!(refused.code.as_str(), "broker.facts_changed");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(reads.load(Ordering::SeqCst), 2, "one preparation and one check, no retry");
    assert_eq!(
        fixture.query::<String>("SELECT request_key FROM operation_idempotency"),
        ["unreadable"]
    );
}

#[tokio::test]
async fn facts_that_never_settle_end_in_a_retryable_refusal_after_three_retries() {
    let fixture = Fixture::new();
    let (reads, calls) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let started = Instant::now();
    let refused = fixture
        .run("never-settles", OnChangedFacts::Retry, never_settling(&reads), &calls)
        .await
        .expect_err("facts that keep changing refuse the command");
    let waited = started.elapsed();
    assert_eq!(refused.code.as_str(), "broker.facts_changed");
    assert_eq!(refused.category, aura_domain_common::ErrorCategory::Conflict);
    assert!(refused.retryable, "nothing ran, so the user may try again");
    assert!(refused.message.contains("没有执行"), "{}", refused.message);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "no attempt reaches the handler");
    assert_eq!(reads.load(Ordering::SeqCst), 8, "four attempts, each prepared and checked");
    assert!(
        waited >= Duration::from_millis(500),
        "the retries wait 50, 150 and 300 ms: {waited:?}"
    );
    assert!(waited < Duration::from_secs(3), "the retries stay bounded: {waited:?}");
    assert_eq!(
        fixture
            .query::<String>("SELECT request_key FROM operation_idempotency ORDER BY request_key"),
        [
            "never-settles",
            "never-settles#retry-1",
            "never-settles#retry-2",
            "never-settles#retry-3"
        ]
    );
    assert_eq!(
        fixture.query::<String>(&format!("SELECT {RESULT} FROM execution_results")),
        ["failed no_effect execution.not_dispatched"; 4],
        "every refused attempt is recorded as not run"
    );
    assert_eq!(fixture.settlements.pending(), 0);
}
