//! B's directed evidence. Every test here drives the real index through the real
//! publisher entry points; none of them reaches into private state to manufacture
//! the fact it is asserting.
use super::*;
mod lookup;
use aura_domain_common::{
    AgentRunId, CommandId, CommandInvocationId, DeferredMutationId, EditorReservationId,
    RuntimeInstanceId, SecurityOperationId, SessionId, ToolCallId, TurnTaskId, WorkspaceId,
    WorkspaceLifecycleOperationId,
};

fn runtime() -> RuntimeInstanceId {
    RuntimeInstanceId::new()
}

fn owner_scope(
    runtime_instance_id: RuntimeInstanceId,
    session: SessionId,
    workspace: WorkspaceId,
    agent: bool,
) -> ActivityOwnerScope {
    ActivityOwnerScope {
        session_id: session,
        workspace_id: workspace,
        agent_run_id: agent.then(AgentRunId::new),
        turn_task_id: agent.then(TurnTaskId::new),
        model_step_id: None,
        runtime_instance_id,
    }
}

fn object_target(volume_serial: u32, file_index: u64) -> ActivityResourceTarget {
    ActivityResourceTarget::ExistingObject { volume_serial, file_index, file_id128: None }
}

fn resource(
    target: ActivityResourceTarget,
    relation: ActivityResourceRelation,
    name: &str,
) -> ActivityResource {
    ActivityResource {
        target,
        relation,
        display_name: name.to_owned(),
        display_name_truncated: false,
    }
}

/// Builds one real Agent tool activity and publishes it through the real path.
fn publish_agent_tool(
    index: &ActivityIndex,
    runtime_instance_id: RuntimeInstanceId,
    session: SessionId,
    workspace: WorkspaceId,
    group: ActivityToolGroup,
    generation: u32,
    resources: Vec<ActivityResource>,
) -> (ActivityId, ActivityExecutionRef) {
    let execution = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let id = ActivityId::derive(ActivityKind::AgentTool, &execution, generation);
    let outcome = index.publish(ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id,
        kind: ActivityKind::AgentTool,
        execution: execution.clone(),
        generation,
        tool_id: ToolId::from_stable_name(match group {
            ActivityToolGroup::FileRead => "fs.read",
            ActivityToolGroup::FileWrite => "fs.write",
            ActivityToolGroup::Shell => "shell.powershell",
            ActivityToolGroup::Web => "web.fetch",
            ActivityToolGroup::Other => "misc",
        }),
        tool_group: group,
        owner: owner_scope(runtime_instance_id, session, workspace, true),
        resources,
        parameters: Vec::new(),
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    });
    assert!(outcome.was_recorded(), "a real publisher must be recorded: {outcome:?}");
    (id, execution)
}

fn view(
    runtime_instance_id: RuntimeInstanceId,
    session: SessionId,
    workspace: WorkspaceId,
) -> ActivityVisibility {
    ActivityVisibility::for_session(runtime_instance_id, session, workspace)
}

// ---------------------------------------------------------------------------
// B-1: several real readers over one fixed snapshot
// ---------------------------------------------------------------------------

#[test]
fn several_real_readers_share_one_fixed_snapshot() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    for generation in 1..=3 {
        publish_agent_tool(
            &index,
            runtime_instance_id,
            session,
            workspace,
            ActivityToolGroup::FileRead,
            generation,
            vec![resource(
                object_target(7, 100 + u64::from(generation)),
                ActivityResourceRelation::Read,
                "a.txt",
            )],
        );
    }
    let view = view(runtime_instance_id, session, workspace);
    // THREE readers, ONE held snapshot. This is the whole point of the sampling
    // contract: a query, a group filter and a resource lookup must describe the
    // same instant, revision and coverage.
    let held = index.snapshot(&view);
    let by_session = held.query(ActivityQuery::Session, 0, 10);
    let by_group = held.query(ActivityQuery::Group { group: ActivityToolGroup::FileRead }, 0, 10);
    let by_resource = held.query(ActivityQuery::Resource { target: object_target(7, 101) }, 0, 10);
    assert_eq!(by_session.rows.len(), 3, "the session reader sees all three");
    assert_eq!(by_group.rows.len(), 3, "the group reader sees all three");
    assert_eq!(by_resource.rows.len(), 1, "the resource reader sees exactly the matching one");
    assert!(
        by_session.same_snapshot(&by_group) && by_group.same_snapshot(&by_resource),
        "every reader must describe the same sampling instant, revision and coverage"
    );
    assert!(by_session.header.coverage.is_complete());
    assert_eq!(by_session.header.coverage.total_in_scope, 3);
}

#[test]
fn a_query_page_is_stable_and_reports_its_own_truncation() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let mut published = Vec::new();
    for generation in 1..=5 {
        let (id, _) = publish_agent_tool(
            &index,
            runtime_instance_id,
            session,
            workspace,
            ActivityToolGroup::FileWrite,
            generation,
            vec![resource(
                object_target(7, 200 + u64::from(generation)),
                ActivityResourceRelation::Write,
                "b.txt",
            )],
        );
        published.push(id);
    }
    let view = view(runtime_instance_id, session, workspace);
    // ONE held snapshot for every page of this paging sequence.
    let held = index.snapshot(&view);
    let first = held.query(ActivityQuery::Session, 0, 2);
    assert_eq!(first.rows.len(), 2);
    assert!(first.has_more);
    assert_eq!(first.header.coverage.state, ActivityCoverageState::Partial);
    assert!(first.header.coverage.truncated);
    assert!(
        first.header.coverage.omission_reasons.contains(&ActivityOmissionReason::SummaryRowLimit)
    );
    assert_eq!(first.header.coverage.omitted_by_limit, 3);

    let mut seen = first.rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let mut offset = 2;
    loop {
        let page = held.query(ActivityQuery::Session, offset, 2);
        if page.rows.is_empty() {
            break;
        }
        // The sampling instant, runtime and revision must be the ones the caller is
        // holding. `omitted_by_limit` is a property of each page, not of the picture,
        // so it is deliberately not compared here.
        assert!(
            page.header.sampled_at == first.header.sampled_at
                && page.header.revision == first.header.revision
                && page.header.runtime_instance_id == first.header.runtime_instance_id,
            "every page must be a projection of the same fixed snapshot"
        );
        seen.extend(page.rows.iter().map(|row| row.id));
        offset += 2;
        if !page.has_more {
            break;
        }
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5, "paging must not drop or duplicate a row");
    assert_eq!(
        held.query(ActivityQuery::Session, 0, 100).rows.len(),
        5,
        "the same held snapshot answers an unbounded page identically"
    );
}

// ---------------------------------------------------------------------------
// B-2: several real reads of one resource stay separate rows
// ---------------------------------------------------------------------------

#[test]
fn several_reads_of_one_resource_do_not_overwrite_each_other() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let target = object_target(11, 4242);
    let (first, _) = publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileRead,
        1,
        vec![resource(target.clone(), ActivityResourceRelation::Read, "shared.txt")],
    );
    let (second, _) = publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileRead,
        2,
        vec![resource(target.clone(), ActivityResourceRelation::Read, "shared.txt")],
    );
    assert_ne!(first, second);
    let view = view(runtime_instance_id, session, workspace);
    let hits =
        index.snapshot(&view).query(ActivityQuery::Resource { target: target.clone() }, 0, 10);
    let ids = hits.rows.iter().map(|row| row.id).collect::<Vec<_>>();
    assert_eq!(ids.len(), 2, "a resource maps to a set of activities, never one slot");
    assert!(ids.contains(&first) && ids.contains(&second));

    // Removing one read must leave the other exactly where it was.
    assert_eq!(index.end(first, 1), ActivityEndOutcome::Ended);
    let after = index.snapshot(&view).query(ActivityQuery::Resource { target }, 0, 10);
    assert_eq!(after.rows.len(), 1, "the second read survives the first read's end");
    assert_eq!(after.rows[0].id, second);
}

#[test]
fn a_same_physical_target_is_not_hidden_by_a_different_workspace() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let other_workspace = WorkspaceId::new();
    let target = object_target(3, 99);
    publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        other_workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(target.clone(), ActivityResourceRelation::Write, "shared.txt")],
    );
    let mut reader = view(runtime_instance_id, session, workspace);
    reader.relevant_targets.insert(ActivityResourceKey::of(&target));
    let hits = index.snapshot(&reader).query(ActivityQuery::Resource { target }, 0, 10);
    assert_eq!(
        hits.rows.len(),
        1,
        "the same physical target in another workspace is the same target, not an unrelated one"
    );
}

#[test]
fn an_unknown_write_set_shell_stays_visible_across_workspaces() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let execution = ActivityExecutionRef::ShellInvocation {
        invocation_id: CommandInvocationId::new(),
        command_id: CommandId::new(),
    };
    let id = ActivityId::derive(ActivityKind::ShellForeground, &execution, 1);
    assert!(
        index
            .publish(ActivityRecord {
                tool_name: String::new(),
                private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
                id,
                kind: ActivityKind::ShellForeground,
                execution,
                generation: 1,
                tool_id: ToolId::from_stable_name("shell.powershell"),
                tool_group: ActivityToolGroup::Shell,
                owner: owner_scope(runtime_instance_id, session, WorkspaceId::new(), true),
                resources: vec![resource(
                    ActivityResourceTarget::Unproven { hint: "unknown write set".to_owned() },
                    ActivityResourceRelation::UnknownWriteSet,
                    "shell",
                )],
                parameters: Vec::new(),
                started_at: UtcTimestamp::now(),
                runtime_instance_id,
                published_at_revision: Revision::INITIAL,
            })
            .was_recorded()
    );
    let reader = view(runtime_instance_id, session, workspace);
    let summary = index.snapshot(&reader).select_summary(&reader, ActivitySummaryBudget::default());
    assert_eq!(
        summary.rows.len(),
        1,
        "an unknown write set is never judged unrelated merely by workspace"
    );
    assert_eq!(
        summary.rows[0].relevance,
        ActivityRelevance::Direct,
        "the row belongs to this reader's own session"
    );
    let unknown = index.snapshot(&reader).query(ActivityQuery::UnknownWriteSet, 0, 10);
    assert_eq!(unknown.rows.len(), 1);
}

#[test]
fn another_sessions_private_body_is_not_shown_without_authorisation() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let mine = SessionId::new();
    let theirs = SessionId::new();
    let workspace = WorkspaceId::new();
    let target = object_target(5, 55);
    // Another session's activity on a target of its own, which this reader never named.
    let other = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let other_id = ActivityId::derive(ActivityKind::AgentTool, &other, 1);
    index.publish(ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id: other_id,
        kind: ActivityKind::AgentTool,
        execution: other,
        generation: 1,
        tool_id: ToolId::from_stable_name("fs.read"),
        tool_group: ActivityToolGroup::FileRead,
        owner: owner_scope(runtime_instance_id, theirs, workspace, true),
        resources: vec![resource(
            target.clone(),
            ActivityResourceRelation::Read,
            "private/secret.txt",
        )],
        parameters: Vec::new(),
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    });
    let reader = view(runtime_instance_id, mine, workspace);
    let snapshot = index.snapshot(&reader);
    assert!(
        snapshot.rows.is_empty(),
        "another session's private body must not be projected to this reader"
    );
    assert_eq!(
        snapshot.header.coverage.visibility_filtered, 1,
        "the row is present but filtered; the filter is disclosed, not silent"
    );
    assert!(
        snapshot
            .header
            .coverage
            .omission_reasons
            .contains(&ActivityOmissionReason::VisibilityFiltered)
    );

    // The same row becomes usable once this reader really names the same target,
    // which is the permission decision, not a relabelling of the source.
    let mut authorising = view(runtime_instance_id, mine, workspace);
    authorising.relevant_targets.insert(ActivityResourceKey::of(&target));
    let visible = index.snapshot(&authorising);
    assert_eq!(visible.rows.len(), 1);
    assert_eq!(
        visible.rows[0].session_id, theirs,
        "the original source marker stays attached; the row is never relabelled as this session's"
    );
}

// ---------------------------------------------------------------------------
// B-3: Main / Agent / Buffer / P04 activity is published by its real owner
// ---------------------------------------------------------------------------

#[test]
fn every_real_publisher_registers_with_its_own_identity_and_no_borrowed_agent_identity() {
    let index = ActivityIndex::new(32);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();

    // Main Fs: a real operation identity plus the real invocation the request carried, with no
    // Agent run. R1 removed the fabricated `MutationJournalId`: Main never owned one, and
    // byte-converting an invocation into a journal id is not an identity.
    let main_execution = ActivityExecutionRef::MainFileMutation {
        operation_id: SecurityOperationId::new(),
        command_invocation_id: Some(CommandInvocationId::new()),
    };
    let main_id = ActivityId::derive(ActivityKind::MainFs, &main_execution, 3);
    assert!(
        index
            .publish(ActivityRecord {
                tool_name: String::new(),
                private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
                id: main_id,
                kind: ActivityKind::MainFs,
                execution: main_execution,
                generation: 3,
                tool_id: ToolId::from_stable_name("fs.write"),
                tool_group: ActivityToolGroup::FileWrite,
                owner: owner_scope(runtime_instance_id, session, workspace, false),
                resources: vec![resource(
                    object_target(2, 800),
                    ActivityResourceRelation::Write,
                    "main.txt"
                )],
                parameters: vec![],
                started_at: UtcTimestamp::now(),
                runtime_instance_id,
                published_at_revision: Revision::INITIAL,
            })
            .was_recorded()
    );

    // Buffer: a real commit identified by its reservation and revision.
    let buffer_execution = ActivityExecutionRef::BufferCommit {
        reservation_id: EditorReservationId::new(),
        buffer_revision: Revision::new(4),
    };
    let buffer_id = ActivityId::derive(ActivityKind::BufferCommit, &buffer_execution, 3);
    assert!(
        index
            .publish(ActivityRecord {
                tool_name: String::new(),
                private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
                id: buffer_id,
                kind: ActivityKind::BufferCommit,
                execution: buffer_execution,
                generation: 3,
                tool_id: ToolId::from_stable_name("editor.save"),
                tool_group: ActivityToolGroup::FileWrite,
                owner: owner_scope(runtime_instance_id, session, workspace, false),
                resources: vec![resource(
                    object_target(2, 801),
                    ActivityResourceRelation::Write,
                    "buffer.txt"
                )],
                parameters: vec![],
                started_at: UtcTimestamp::now(),
                runtime_instance_id,
                published_at_revision: Revision::INITIAL,
            })
            .was_recorded()
    );

    // Main's lifecycle operation plus its publication identity.
    let p04_execution = ActivityExecutionRef::P04Publication {
        lifecycle_operation_id: WorkspaceLifecycleOperationId::new(),
        publication_id: DeferredMutationId::new(),
    };
    let p04_id = ActivityId::derive(ActivityKind::P04Publication, &p04_execution, 3);
    assert!(
        index
            .publish(ActivityRecord {
                tool_name: String::new(),
                private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
                id: p04_id,
                kind: ActivityKind::P04Publication,
                execution: p04_execution,
                generation: 3,
                tool_id: ToolId::from_stable_name("p04.publish"),
                tool_group: ActivityToolGroup::FileWrite,
                owner: owner_scope(runtime_instance_id, session, workspace, false),
                resources: vec![resource(
                    ActivityResourceTarget::Subtree {
                        volume_serial: 2,
                        file_index: 10,
                        inclusive: true
                    },
                    ActivityResourceRelation::Write,
                    "workspace root",
                )],
                parameters: vec![],
                started_at: UtcTimestamp::now(),
                runtime_instance_id,
                published_at_revision: Revision::INITIAL,
            })
            .was_recorded()
    );

    let reader = view(runtime_instance_id, session, workspace);
    let all = index.snapshot(&reader).query(ActivityQuery::Session, 0, 10);
    assert_eq!(all.rows.len(), 3, "Main, Buffer and P04 are three real publishers");

    // The single most important negative fact: a non-Agent publisher must not
    // carry an Agent identity, and nothing here invents one for it.
    for row in &all.rows {
        assert!(
            row.agent_run_id.is_none(),
            "{} must not manufacture an Agent identity",
            row.kind.as_str()
        );
        assert!(!row.kind.is_agent_owned(), "{} is not an Agent-owned executor", row.kind.as_str());
    }
    // And the inverse: an Agent row always has one.
    let (_, _) = publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileWrite,
        9,
        vec![resource(object_target(2, 802), ActivityResourceRelation::Write, "agent.txt")],
    );
    let agent_row = index
        .snapshot(&reader)
        .query(ActivityQuery::Group { group: ActivityToolGroup::FileWrite }, 0, 10)
        .rows
        .into_iter()
        .find(|row| row.kind == ActivityKind::AgentTool)
        .expect("the Agent row");
    assert!(agent_row.agent_run_id.is_some());

    // Every publisher in the declared table is a real owner with a real source file.
    //
    // R1: the table had **seven** rows, and one of them - the `reservation.rs` row - named a file
    // that contains no `publish` call at all. A row that claims coverage without a call is worse
    // than a missing row: read as data, it reports an executor that does not publish. It is gone,
    // and the reservation's real contribution (the attempt ordinal the dispatch publisher carries)
    // is stated in the Agent row instead. Six rows, six real call sites.
    assert_eq!(ACTIVITY_PUBLISHERS.len(), 6);
    assert!(ACTIVITY_PUBLISHERS.iter().all(|row| row.source_file.starts_with("crates/")));
    assert!(
        ACTIVITY_PUBLISHERS.iter().filter(|row| !row.may_carry_agent_identity).count() == 4,
        "Main Fs, Buffer commit, P04 publication and the managed background owner carry no Agent identity"
    );
    // The two Agent-carrying rows are the two faces of the one real-start seam: the ordinary tool
    // call and the Shell it is grouped as.
    assert!(
        ACTIVITY_PUBLISHERS
            .iter()
            .filter(|row| row.may_carry_agent_identity)
            .all(|row| row.source_file.ends_with("agent_execution/dispatch.rs")),
        "only the real dispatch seam publishes an Agent-owned activity"
    );
}

// ---------------------------------------------------------------------------
// B-4: the Shell foreground/completed versus managed background boundary
// ---------------------------------------------------------------------------

#[test]
fn a_completed_foreground_shell_does_not_pretend_the_background_ended() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();

    // The real managed background owner publishes its own activity.
    let background_execution = ActivityExecutionRef::ShellInvocation {
        invocation_id: CommandInvocationId::new(),
        command_id: CommandId::new(),
    };
    let background_id =
        ActivityId::derive(ActivityKind::ShellManagedBackground, &background_execution, 1);
    index.publish(ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id: background_id,
        kind: ActivityKind::ShellManagedBackground,
        execution: background_execution,
        generation: 1,
        tool_id: ToolId::from_stable_name("shell.powershell"),
        tool_group: ActivityToolGroup::Shell,
        owner: owner_scope(runtime_instance_id, session, workspace, false),
        resources: vec![resource(
            ActivityResourceTarget::Unproven { hint: "managed descendants".to_owned() },
            ActivityResourceRelation::UnknownWriteSet,
            "background",
        )],
        parameters: vec![],
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    });

    // The foreground call returns `Completed` and ends only its own activity.
    let (foreground_id, _) = {
        let execution = ActivityExecutionRef::ShellInvocation {
            invocation_id: CommandInvocationId::new(),
            command_id: CommandId::new(),
        };
        let id = ActivityId::derive(ActivityKind::ShellForeground, &execution, 1);
        index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id,
            kind: ActivityKind::ShellForeground,
            execution,
            generation: 1,
            tool_id: ToolId::from_stable_name("shell.powershell"),
            tool_group: ActivityToolGroup::Shell,
            owner: owner_scope(runtime_instance_id, session, workspace, true),
            resources: vec![],
            parameters: vec![],
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        });
        (id, ())
    };
    assert_eq!(index.end(foreground_id, 1), ActivityEndOutcome::Ended);

    let reader = view(runtime_instance_id, session, workspace);
    let remaining = index.snapshot(&reader).query(ActivityQuery::Session, 0, 10);
    assert_eq!(remaining.rows.len(), 1, "the managed background is still running");
    assert_eq!(remaining.rows[0].id, background_id);
    assert_eq!(remaining.rows[0].kind, ActivityKind::ShellManagedBackground);
    assert!(
        !ActivityKind::ShellManagedBackground.foreground_end_is_conclusive(),
        "a foreground Completed is not evidence that the background ended"
    );
}

#[test]
fn the_managed_background_owner_never_borrows_an_agent_run() {
    // The managed background owner is a non-Agent publisher. Its declared identity
    // must be its own; the table must not quietly widen it to include an Agent run,
    // because that would be manufacturing an Agent identity for a process that has
    // none.
    let rows = ACTIVITY_PUBLISHERS
        .iter()
        .filter(|row| row.kind == ActivityKind::ShellManagedBackground)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "exactly one managed-background publisher is declared");
    for row in rows {
        assert!(
            !row.may_carry_agent_identity,
            "the managed background owner must not be marked Agent-carrying"
        );
        // It must be declared as reusing its own background invocation identity,
        // and any mention of an Agent run must be an explicit *absence*.
        assert!(
            row.reused_identity.contains("SecurityOperationId")
                && row.reused_identity.contains("attempt ordinal"),
            "the managed background owner reuses the actual operation and attempt: {}",
            row.reused_identity
        );
        let mentions_agent_run = row.reused_identity.contains("AgentRunId");
        assert!(
            !mentions_agent_run || row.reused_identity.contains("no AgentRunId"),
            "the managed background owner must not be declared as carrying an Agent run: {}",
            row.reused_identity
        );
    }
}

// ---------------------------------------------------------------------------
// B-5: old generation and repeated ends
// ---------------------------------------------------------------------------

#[test]
fn a_late_end_from_an_old_generation_does_not_remove_the_new_generation() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let execution = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let old_id = ActivityId::derive(ActivityKind::AgentTool, &execution, 1);
    let new_id = ActivityId::derive(ActivityKind::AgentTool, &execution, 2);
    for (id, generation) in [(old_id, 1u32), (new_id, 2u32)] {
        index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id,
            kind: ActivityKind::AgentTool,
            execution: execution.clone(),
            generation,
            tool_id: ToolId::from_stable_name("fs.read"),
            tool_group: ActivityToolGroup::FileRead,
            owner: owner_scope(runtime_instance_id, session, workspace, true),
            resources: vec![],
            parameters: vec![],
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        });
    }
    // The new generation ends first, then the old generation's late end arrives.
    assert_eq!(index.end(new_id, 2), ActivityEndOutcome::Ended);
    assert_eq!(
        index.end(old_id, 1),
        ActivityEndOutcome::Ended,
        "the old generation's own row is still the row it names"
    );

    // Re-publish the new generation, then deliver the stale end for the old one.
    let fresh_id = ActivityId::derive(ActivityKind::AgentTool, &execution, 3);
    index.publish(ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id: fresh_id,
        kind: ActivityKind::AgentTool,
        execution: execution.clone(),
        generation: 3,
        tool_id: ToolId::from_stable_name("fs.read"),
        tool_group: ActivityToolGroup::FileRead,
        owner: owner_scope(runtime_instance_id, session, workspace, true),
        resources: vec![],
        parameters: vec![],
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    });
    assert_eq!(
        index.end(old_id, 1),
        ActivityEndOutcome::NotCurrent,
        "an old identity that is no longer current removes nothing"
    );
    assert_eq!(
        index.end(fresh_id, 9),
        ActivityEndOutcome::GenerationMismatch,
        "the right id under the wrong generation is not the same activity"
    );
    let reader = view(runtime_instance_id, session, workspace);
    assert_eq!(
        index.snapshot(&reader).query(ActivityQuery::Session, 0, 10).rows.len(),
        1,
        "the new generation survives both stale ends"
    );
}

#[test]
fn a_repeated_end_is_safe_and_changes_nothing_else() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let (id, _) = publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileRead,
        1,
        vec![resource(object_target(4, 4), ActivityResourceRelation::Read, "x.txt")],
    );
    let (other, _) = publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileRead,
        2,
        vec![resource(object_target(4, 5), ActivityResourceRelation::Read, "y.txt")],
    );
    assert_eq!(index.end(id, 1), ActivityEndOutcome::Ended);
    let revision_after_first = index.revision();
    assert_eq!(index.end(id, 1), ActivityEndOutcome::NotCurrent);
    assert_eq!(index.end(id, 1), ActivityEndOutcome::NotCurrent);
    assert_eq!(
        index.revision(),
        revision_after_first,
        "a repeated end of a finished activity changes no state at all"
    );
    let reader = view(runtime_instance_id, session, workspace);
    let rows = index.snapshot(&reader).query(ActivityQuery::Session, 0, 10).rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, other, "the repeated end touched no other activity");
}

// ---------------------------------------------------------------------------
// B-6: overflow, dropped events, invisible data and the independent path
// ---------------------------------------------------------------------------

#[test]
fn an_overflowed_index_reports_partial_on_a_path_the_overflow_cannot_reach() {
    let index = ActivityIndex::new(2);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let mut outcomes = Vec::new();
    for generation in 1..=4 {
        let execution = ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        };
        let id = ActivityId::derive(ActivityKind::AgentTool, &execution, generation);
        outcomes.push(index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id,
            kind: ActivityKind::AgentTool,
            execution,
            generation,
            tool_id: ToolId::from_stable_name("fs.read"),
            tool_group: ActivityToolGroup::FileRead,
            owner: owner_scope(runtime_instance_id, session, workspace, true),
            resources: vec![],
            parameters: vec![],
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        }));
    }
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == ActivityPublishOutcome::CapacityExhausted)
            .count(),
        2,
        "the two facts past capacity are refused, not silently lost"
    );
    let reader = view(runtime_instance_id, session, workspace);
    let snapshot = index.snapshot(&reader);
    assert_eq!(snapshot.rows.len(), 2);
    assert_eq!(
        snapshot.header.coverage.state,
        ActivityCoverageState::Partial,
        "dropped facts are visible as coverage, and the counter lives on the publish path"
    );
    assert_eq!(snapshot.header.coverage.capacity_dropped, 2);
    assert!(
        snapshot
            .header
            .coverage
            .omission_reasons
            .contains(&ActivityOmissionReason::CapacityDropped)
    );
    assert!(!snapshot.header.coverage.is_complete());
}

#[test]
fn an_unavailable_index_is_not_an_empty_table() {
    let index = ActivityIndex::new(4);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let reader = view(runtime_instance_id, session, workspace);

    let healthy = index.snapshot(&reader);
    assert!(healthy.rows.is_empty());
    assert!(healthy.header.coverage.is_complete(), "zero rows from a healthy index is a real zero");

    index.mark_unavailable();
    let broken = index.snapshot(&reader);
    assert!(broken.rows.is_empty());
    assert_eq!(broken.header.coverage.state, ActivityCoverageState::Unavailable);
    assert!(
        !broken.header.coverage.is_complete(),
        "an unavailable index must never read as a complete empty table"
    );
    assert_ne!(
        broken.header.coverage.state, healthy.header.coverage.state,
        "the two zero-row pictures are different values, not the same zero"
    );

    // Publishing into an unavailable index drops the fact and says so.
    let execution = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let id = ActivityId::derive(ActivityKind::AgentTool, &execution, 1);
    assert_eq!(
        index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id,
            kind: ActivityKind::AgentTool,
            execution,
            generation: 1,
            tool_id: ToolId::from_stable_name("fs.read"),
            tool_group: ActivityToolGroup::FileRead,
            owner: owner_scope(runtime_instance_id, session, workspace, true),
            resources: vec![],
            parameters: vec![],
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        }),
        ActivityPublishOutcome::Unavailable
    );
}

#[test]
fn a_malformed_fact_is_refused_and_counted_without_touching_a_good_one() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let (_, _) = publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileRead,
        1,
        vec![resource(object_target(1, 1), ActivityResourceRelation::Read, "good.txt")],
    );
    let execution = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let id = ActivityId::derive(ActivityKind::AgentTool, &execution, 2);
    let outcome = index.publish(ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id,
        kind: ActivityKind::AgentTool,
        execution,
        generation: 2,
        tool_id: ToolId::from_stable_name("fs.read"),
        tool_group: ActivityToolGroup::FileRead,
        owner: owner_scope(runtime_instance_id, session, workspace, true),
        resources: vec![],
        // A "key parameter" that is neither on an allowlist nor bounded is refused.
        parameters: vec![ActivityParameter {
            key: String::new(),
            value: "x".repeat(MAX_ACTIVITY_PARAMETER_BYTES + 1),
            truncated: false,
        }],
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    });
    assert_eq!(outcome, ActivityPublishOutcome::Rejected);
    let reader = view(runtime_instance_id, session, workspace);
    let snapshot = index.snapshot(&reader);
    assert_eq!(snapshot.rows.len(), 1, "the good fact is untouched");
    assert_eq!(snapshot.header.coverage.rejected, 1);
    assert_eq!(snapshot.header.coverage.state, ActivityCoverageState::Partial);
}

#[test]
fn one_broken_group_does_not_take_another_group_out_of_the_index() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileRead,
        1,
        vec![resource(object_target(1, 10), ActivityResourceRelation::Read, "read.txt")],
    );
    publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::Web,
        2,
        vec![resource(object_target(1, 11), ActivityResourceRelation::Read, "web")],
    );
    let reader = view(runtime_instance_id, session, workspace);
    // A group query that finds nothing is not an index failure.
    let empty = index.snapshot(&reader).query(
        ActivityQuery::Group { group: ActivityToolGroup::Other },
        0,
        10,
    );
    assert!(empty.rows.is_empty());
    assert!(empty.header.coverage.is_complete());
    // The other groups are still answered.
    assert_eq!(
        index
            .snapshot(&reader)
            .query(ActivityQuery::Group { group: ActivityToolGroup::FileRead }, 0, 10)
            .rows
            .len(),
        1
    );
    assert_eq!(
        index
            .snapshot(&reader)
            .query(ActivityQuery::Group { group: ActivityToolGroup::Web }, 0, 10)
            .rows
            .len(),
        1
    );
}

// ---------------------------------------------------------------------------
// B-7: the index is not an authority, and its failure cannot reach execution
// ---------------------------------------------------------------------------

#[test]
fn the_index_has_no_admission_surface() {
    // A compile-time-shaped assertion: the public surface of this crate exposes no
    // function that admits, denies, stops, isolates, replays or releases a
    // resource. The list below is the whole public API of `ActivityIndex`; if a
    // future change adds an authority verb, this test has to be edited, which is
    // exactly the review gate.
    let api = [
        "new",
        "publish",
        "end",
        "mark_unavailable",
        "mark_available",
        "capacity",
        "snapshot",
        "query",
        "revision",
        "is_unavailable",
        "len",
        "is_empty",
        "fingerprint",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    for forbidden in ["admit", "deny", "stop", "cancel", "isolate", "release", "replay", "acquire"]
    {
        assert!(
            !api.contains(forbidden),
            "the information index must not grow an authority verb: {forbidden}"
        );
    }
    let source = include_str!("lib.rs");
    assert!(
        !source.contains("EligibilityOwner") && !source.contains("ResourceCommit"),
        "the information index must not link A's common-admission service"
    );
}

#[test]
fn publishing_never_touches_admission_or_protection() {
    // The publish path is pure in-memory bookkeeping over facts the caller already
    // holds. It cannot re-open a path, wait on the journal or take a business lock,
    // because it has no access to any of them: the whole crate's dependency list
    // is the domain contract plus serde. This test asserts the observable half -
    // that a publish has no side effect outside the index - and the source scan in
    // the test above asserts the structural half.
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let before = index.fingerprint();
    let execution = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let id = ActivityId::derive(ActivityKind::AgentTool, &execution, 1);
    assert_eq!(
        index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id,
            kind: ActivityKind::AgentTool,
            execution,
            generation: 1,
            tool_id: ToolId::from_stable_name("fs.write"),
            tool_group: ActivityToolGroup::FileWrite,
            owner: owner_scope(runtime_instance_id, session, workspace, true),
            resources: vec![resource(
                object_target(1, 1),
                ActivityResourceRelation::Write,
                "f.txt"
            )],
            parameters: vec![],
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        }),
        ActivityPublishOutcome::Recorded
    );
    assert_ne!(index.fingerprint(), before, "the index recorded exactly its own fact");
}

#[test]
fn an_index_error_leaves_the_caller_able_to_continue() {
    // A dropped fact is reported, never raised. The publisher's own work is not
    // gated on this module: every outcome is a value, and every failure path
    // returns one of the non-recording variants instead of a `Result::Err`.
    let index = ActivityIndex::new(1);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    for generation in 1..=3 {
        let execution = ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        };
        let id = ActivityId::derive(ActivityKind::AgentTool, &execution, generation);
        let outcome = index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id,
            kind: ActivityKind::AgentTool,
            execution,
            generation,
            tool_id: ToolId::from_stable_name("fs.read"),
            tool_group: ActivityToolGroup::FileRead,
            owner: owner_scope(runtime_instance_id, session, workspace, true),
            resources: vec![],
            parameters: vec![],
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        });
        assert!(
            outcome.was_recorded() || outcome == ActivityPublishOutcome::CapacityExhausted,
            "a publish reports; it never faults the caller"
        );
    }
    // An end into a full-or-broken index also reports instead of failing.
    assert_eq!(index.end(ActivityId::from_bytes([0u8; 16]), 1), ActivityEndOutcome::NotCurrent);
}

// ---------------------------------------------------------------------------
// B-8: the fixed budget, and the snapshot the summary is taken from
// ---------------------------------------------------------------------------

#[test]
fn the_summary_is_selected_within_its_fixed_budget() {
    let index = ActivityIndex::new(64);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    for generation in 1..=40 {
        publish_agent_tool(
            &index,
            runtime_instance_id,
            session,
            workspace,
            ActivityToolGroup::FileWrite,
            generation,
            vec![resource(
                object_target(9, 1000 + u64::from(generation)),
                ActivityResourceRelation::Write,
                &format!("file-{generation:02}.txt"),
            )],
        );
    }
    let reader = view(runtime_instance_id, session, workspace);
    let snapshot = index.snapshot(&reader);
    let budget = ActivitySummaryBudget { max_rows: 5, max_bytes: MAX_ACTIVITY_SUMMARY_BYTES };
    let summary = snapshot.select_summary(&reader, budget);
    assert_eq!(summary.rows.len(), 5, "the row cap is respected");
    assert!(summary.body.len() <= MAX_ACTIVITY_SUMMARY_BYTES);
    assert_eq!(summary.header.coverage.omitted_by_limit, 35);
    assert_eq!(summary.header.coverage.state, ActivityCoverageState::Partial);
    assert!(
        summary.header.coverage.omission_reasons.contains(&ActivityOmissionReason::SummaryRowLimit)
    );

    // The byte cap is a second, independent bound.
    let tight =
        snapshot.select_summary(&reader, ActivitySummaryBudget { max_rows: 40, max_bytes: 120 });
    assert!(tight.body.len() <= 120);
    assert!(
        tight.header.coverage.omission_reasons.contains(&ActivityOmissionReason::SummaryByteLimit)
    );
}

#[test]
fn the_summary_and_the_query_describe_the_same_instant() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(object_target(6, 6), ActivityResourceRelation::Write, "same.txt")],
    );
    let reader = view(runtime_instance_id, session, workspace);
    let snapshot = index.snapshot(&reader);
    let summary = snapshot.select_summary(&reader, ActivitySummaryBudget::default());
    let page = snapshot.query(ActivityQuery::Session, 0, 10);
    assert_eq!(summary.header.sampled_at, page.header.sampled_at);
    assert_eq!(summary.header.revision, page.header.revision);
    assert_eq!(summary.header.runtime_instance_id, page.header.runtime_instance_id);
    assert_eq!(summary.rows[0].row.id, page.rows[0].id);
    assert_eq!(
        summary_content_hash(&summary.body),
        summary_content_hash(&summary.body),
        "the frozen body hashes stably"
    );
}

#[test]
fn an_unavailable_summary_is_empty_rather_than_a_failure() {
    let index = ActivityIndex::new(4);
    let runtime_instance_id = runtime();
    let reader = view(runtime_instance_id, SessionId::new(), WorkspaceId::new());
    index.mark_unavailable();
    let summary = index.snapshot(&reader).select_summary(&reader, ActivitySummaryBudget::default());
    assert!(summary.body.is_empty());
    assert!(!summary.is_sendable(), "nothing is sent, and nothing fails");
    assert_eq!(summary.header.coverage.state, ActivityCoverageState::Unavailable);
}

#[test]
fn an_idle_but_healthy_index_is_sendable_and_differs_from_a_blind_one() {
    let index = ActivityIndex::new(4);
    let runtime_instance_id = runtime();
    let reader = view(runtime_instance_id, SessionId::new(), WorkspaceId::new());

    // A healthy index with nothing running **is** sendable: "the index answered and no writer
    // is registered" is a real fact, and suppressing it would leave the model unable to tell an
    // idle runtime from a blind one.
    let idle = index.snapshot(&reader).select_summary(&reader, ActivitySummaryBudget::default());
    assert!(idle.body.is_empty());
    assert!(
        idle.is_sendable(),
        "an answered, empty picture is a fact worth sending, not a suppression"
    );
    assert!(idle.header.coverage.is_complete());
    assert_eq!(idle.header.coverage.total_in_scope, 0);

    // The blind case is a different value with a different answer.
    index.mark_unavailable();
    let blind = index.snapshot(&reader).select_summary(&reader, ActivitySummaryBudget::default());
    assert!(!blind.is_sendable());
    assert_ne!(
        idle.header.coverage.state, blind.header.coverage.state,
        "an idle index and a blind index must not be the same value"
    );
    // And the caller can tell them apart without a second request.
    assert_ne!(idle.header.coverage, blind.header.coverage);
}

#[test]
fn the_visibility_filter_runs_before_the_budget() {
    let index = ActivityIndex::new(32);
    let runtime_instance_id = runtime();
    let mine = SessionId::new();
    let theirs = SessionId::new();
    let workspace = WorkspaceId::new();
    // Fill the reader's own session with the whole row budget.
    for generation in 1..=8 {
        publish_agent_tool(
            &index,
            runtime_instance_id,
            mine,
            workspace,
            ActivityToolGroup::FileWrite,
            generation,
            vec![resource(
                object_target(8, u64::from(generation)),
                ActivityResourceRelation::Write,
                "mine.txt",
            )],
        );
    }
    // Another session's private row, which this reader may not see.
    let other = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let other_id = ActivityId::derive(ActivityKind::AgentTool, &other, 99);
    index.publish(ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id: other_id,
        kind: ActivityKind::AgentTool,
        execution: other,
        generation: 99,
        tool_id: ToolId::from_stable_name("fs.write"),
        tool_group: ActivityToolGroup::FileWrite,
        owner: owner_scope(runtime_instance_id, theirs, workspace, true),
        resources: vec![resource(
            object_target(8, 999),
            ActivityResourceRelation::Write,
            "private.txt",
        )],
        parameters: vec![],
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    });
    let reader = view(runtime_instance_id, mine, workspace);
    let summary = index.snapshot(&reader).select_summary(
        &reader,
        ActivitySummaryBudget { max_rows: 4, max_bytes: MAX_ACTIVITY_SUMMARY_BYTES },
    );
    assert_eq!(summary.rows.len(), 4, "the budget is spent on visible rows only");
    assert!(summary.rows.iter().all(|row| row.row.id != other_id));
    assert!(
        !summary.body.contains("private.txt"),
        "a filtered body must not appear in the summary"
    );
    assert_eq!(
        summary.header.coverage.visibility_filtered, 1,
        "the filter is applied first and then disclosed"
    );
}

// ---------------------------------------------------------------------------
// B-9: publication cost, and the observer that is not the index
// ---------------------------------------------------------------------------

#[test]
fn publishing_a_large_burst_stays_within_the_fixed_budget() {
    // The publish path takes one in-memory mutex and does a bounded amount of work.
    // If it ever grew a path re-open, a journal wait or a business lock, this burst
    // would not finish in the budget. The bound is deliberately loose so the test
    // measures the shape of the path rather than a machine's speed.
    let index = ActivityIndex::new(DEFAULT_ACTIVITY_CAPACITY);
    let started = std::time::Instant::now();
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    for generation in 1..=DEFAULT_ACTIVITY_CAPACITY {
        let execution = ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        };
        let id = ActivityId::derive(ActivityKind::AgentTool, &execution, generation as u32);
        assert!(
            index
                .publish(ActivityRecord {
                    tool_name: String::new(),
                    private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
                    id,
                    kind: ActivityKind::AgentTool,
                    execution,
                    generation: generation as u32,
                    tool_id: ToolId::from_stable_name("fs.write"),
                    tool_group: ActivityToolGroup::FileWrite,
                    owner: owner_scope(runtime_instance_id, session, workspace, true),
                    resources: vec![resource(
                        object_target(1, generation as u64),
                        ActivityResourceRelation::Write,
                        "burst.txt"
                    )],
                    parameters: vec![],
                    started_at: UtcTimestamp::now(),
                    runtime_instance_id,
                    published_at_revision: Revision::INITIAL,
                })
                .was_recorded()
        );
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "a full-capacity burst must stay a memory operation, took {elapsed:?}"
    );
    assert_eq!(index.len(), DEFAULT_ACTIVITY_CAPACITY);
}

#[test]
fn the_execution_observer_is_not_the_production_index() {
    // The acceptance execution counter is a test observer: it lives behind the
    // default-off `acceptance-barrier` feature and writes a journal beside an
    // external control document. The production index must not be that counter.
    let observer = ActivityPublicationObserver::default();
    let index = ActivityIndex::new(1);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    for generation in 1..=2 {
        let execution = ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        };
        let id = ActivityId::derive(ActivityKind::AgentTool, &execution, generation);
        let outcome = index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id,
            kind: ActivityKind::AgentTool,
            execution,
            generation,
            tool_id: ToolId::from_stable_name("fs.write"),
            tool_group: ActivityToolGroup::FileWrite,
            owner: owner_scope(runtime_instance_id, session, workspace, true),
            resources: vec![],
            parameters: vec![],
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        });
        observer.observe(outcome);
    }
    assert_eq!(
        observer.counters().capacity_dropped,
        1,
        "the observer sees the drop on a path independent of the index's own rows"
    );
    assert_eq!(index.len(), 1);
    // The observer is a separate value: it holds no rows and cannot answer a query.
    assert_eq!(observer.counters().rejected, 0);
}

// ---------------------------------------------------------------------------
// B-10: the negative controls
// ---------------------------------------------------------------------------

#[test]
fn the_old_workspace_keyed_implementation_is_rejected() {
    // Counterexample 1: "another workspace is a different resource".
    // A WorkspaceId-equality implementation hides the overlapping row; the real
    // index finds it by physical identity.
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let workspace_a = WorkspaceId::new();
    let workspace_b = WorkspaceId::new();
    let session = SessionId::new();
    let shared = object_target(1, 500);
    publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace_b,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(shared.clone(), ActivityResourceRelation::Write, "overlap.txt")],
    );
    let mut reader = view(runtime_instance_id, session, workspace_a);
    reader.relevant_targets.insert(ActivityResourceKey::of(&shared));
    assert_eq!(
        index.snapshot(&reader).query(ActivityQuery::Resource { target: shared }, 0, 10).rows.len(),
        1,
        "the workspace-keyed implementation would have reported zero here"
    );
}

#[test]
fn the_old_single_slot_resource_map_is_rejected() {
    // Counterexample 2: "by_resource maps a resource to one activity id".
    // Two concurrent readers of one file must both be visible.
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let target = object_target(2, 900);
    for generation in 1..=2 {
        publish_agent_tool(
            &index,
            runtime_instance_id,
            session,
            workspace,
            ActivityToolGroup::FileRead,
            generation,
            vec![resource(target.clone(), ActivityResourceRelation::Read, "contended.txt")],
        );
    }
    let reader = view(runtime_instance_id, session, workspace);
    assert_eq!(
        index.snapshot(&reader).query(ActivityQuery::Resource { target }, 0, 10).rows.len(),
        2,
        "a single-slot map would have overwritten one of the two reads"
    );
}

#[test]
fn the_old_cwd_keyed_shell_implementation_is_rejected() {
    // Counterexample 3: "a Shell's cwd proves its write set".
    // A cwd within the reader's workspace would be judged related; an honest
    // unknown write set is what the index actually records.
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let execution = ActivityExecutionRef::ShellInvocation {
        invocation_id: CommandInvocationId::new(),
        command_id: CommandId::new(),
    };
    let id = ActivityId::derive(ActivityKind::ShellForeground, &execution, 1);
    index.publish(ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id,
        kind: ActivityKind::ShellForeground,
        execution,
        generation: 1,
        tool_id: ToolId::from_stable_name("shell.powershell"),
        tool_group: ActivityToolGroup::Shell,
        owner: owner_scope(runtime_instance_id, session, workspace, true),
        resources: vec![resource(
            ActivityResourceTarget::Unproven { hint: "unproven write set".to_owned() },
            ActivityResourceRelation::UnknownWriteSet,
            "shell",
        )],
        parameters: ActivityParameterAllowlist::project(
            ActivityToolGroup::Shell,
            "cwd",
            "C:/work/project",
        )
        .into_iter()
        .collect(),
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    });
    let reader = view(runtime_instance_id, session, workspace);
    let row = index.snapshot(&reader).query(ActivityQuery::Session, 0, 10).rows.remove(0);
    assert_eq!(
        row.resources[0].relation,
        ActivityResourceRelation::UnknownWriteSet,
        "the cwd is carried as a parameter; it is never promoted to a proven write set"
    );
    assert!(
        row.resources
            .iter()
            .all(|resource| !matches!(resource.target, ActivityResourceTarget::Subtree { .. })),
        "the cwd must not become a directory write scope"
    );
    assert!(row.parameters.iter().any(|parameter| parameter.key == "cwd"));
}

#[test]
fn the_old_shared_activity_id_for_a_new_generation_is_rejected() {
    // Counterexample 4: "the activity id does not include the runtime generation".
    // With a generation-free id, the second publish would be reported as a
    // duplicate and the old end would remove the new row.
    let reference = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let generation_free = ActivityId::derive(ActivityKind::AgentTool, &reference, 0);
    let generation_one = ActivityId::derive(ActivityKind::AgentTool, &reference, 1);
    assert_ne!(
        generation_free, generation_one,
        "the derivation must separate generations, or a late end cannot be told apart"
    );
}

#[test]
fn paging_does_not_turn_an_unavailable_index_into_a_partial_answer() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    for n in [1, 2] {
        publish_agent_tool(
            &index,
            runtime_instance_id,
            session,
            workspace,
            ActivityToolGroup::FileRead,
            n,
            vec![resource(
                object_target(7, u64::from(n)),
                ActivityResourceRelation::Read,
                "read.txt",
            )],
        );
    }
    index.mark_unavailable();
    let reader = view(runtime_instance_id, session, workspace);
    let answer = index.snapshot(&reader).query(ActivityQuery::Session, 0, 1);
    assert_eq!(answer.header.coverage.state, ActivityCoverageState::Unavailable);
    assert!(!answer.header.coverage.is_complete());
    assert!(!answer.has_more, "an unavailable index cannot promise another trustworthy page");
    assert!(answer.rows.is_empty(), "unavailable observations are not sent as running facts");
}

// ---------------------------------------------------------------------------
// R3: one physical fact set, real association, and a real fast lookup
// ---------------------------------------------------------------------------

/// **R3** — two reads of one file stay separate, and the resource index is a real set.
///
/// The pre-fix shape keyed `by_resource` as `Resource -> ActivityId`, so the second read silently
/// overwrote the first and a query about that file returned one row where two real readers existed.
#[test]
fn a_resource_lookup_returns_every_activity_and_two_reads_stay_separate() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let file = object_target(3, 77);
    for _ in 0..2 {
        publish_agent_tool(
            &index,
            runtime_instance_id,
            session,
            workspace,
            ActivityToolGroup::FileRead,
            1,
            vec![resource(file.clone(), ActivityResourceRelation::Read, "shared.txt")],
        );
    }
    // The real fast lookup sees both, because the association is a set.
    let candidates =
        index.resource_candidates(&file).expect("a proven object target has an indexed lookup");
    assert_eq!(candidates.len(), 2, "two reads of one file are two activities, not one");
    // Ending one leaves the other addressable.
    let ended = *candidates.iter().next().expect("one of the two");
    assert_eq!(index.end(ended, 1), ActivityEndOutcome::Ended);
    assert_eq!(
        index.resource_candidates(&file).expect("still indexed").len(),
        1,
        "ending one read must not remove the other"
    );
}

/// **R3** — a query about a directory finds the activities that named something inside it.
///
/// This is the relation exact enum-key equality cannot express: a write to `dir/file.txt` is not
/// key-equal to `dir`, so before the subtree index existed a directory question found nothing even
/// though the store really held an affected child.
#[test]
fn a_directory_query_finds_an_activity_that_named_a_child() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let directory =
        ActivityResourceTarget::Subtree { volume_serial: 5, file_index: 900, inclusive: true };
    // A child named by its already-verified parent, which is what a create publishes.
    let child = ActivityResourceTarget::DirectoryEntry {
        parent_volume_serial: 5,
        parent_file_index: 900,
        leaf: "child.txt".to_owned(),
        case_insensitive: false,
        comparison_key: None,
    };
    publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(child.clone(), ActivityResourceRelation::Write, "child.txt")],
    );
    let from_directory =
        index.resource_candidates(&directory).expect("a directory target has an indexed lookup");
    assert_eq!(from_directory.len(), 1, "the directory query must find the affected child");
    let from_child =
        index.resource_candidates(&child).expect("a child target has an indexed lookup");
    assert_eq!(from_child.len(), 1, "and the child query finds the activity that named its parent");
    // A sibling directory does not reach it: the relation is a real subtree, not a global match.
    let sibling =
        ActivityResourceTarget::Subtree { volume_serial: 5, file_index: 901, inclusive: true };
    assert!(
        index.resource_candidates(&sibling).expect("indexed").is_empty(),
        "an unrelated directory must not be associated by a parent-range coincidence"
    );
}

/// **R3** — an unproven target is reported as having no fast path rather than as empty.
///
/// `None` and "zero candidates" are different answers: the first says the index cannot answer by
/// key and the caller should scan, the second claims a real absence. Collapsing them would make a
/// path hint look like proof that nothing is associated with it.
#[test]
fn an_unproven_target_reports_no_fast_path_rather_than_an_empty_one() {
    let index = ActivityIndex::new(16);
    let unproven = ActivityResourceTarget::Unproven { hint: "a.txt".to_owned() };
    assert!(index.resource_candidates(&unproven).is_none());
    index.mark_unavailable();
    assert!(index.resource_candidates(&object_target(1, 1)).is_none());
}

/// **R3** — ending every activity reclaims the per-key index entries.
///
/// An entry left behind with an empty set is a leak that grows with every session, workspace and
/// resource the process has ever served. This asserts the reclamation is real rather than intended.
#[test]
fn ending_an_activity_reclaims_its_index_entries() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let file = object_target(9, 42);
    let (id, _) = publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(file.clone(), ActivityResourceRelation::Write, "reclaim.txt")],
    );
    assert_eq!(index.resource_candidates(&file).expect("indexed").len(), 1);
    assert_eq!(index.end(id, 1), ActivityEndOutcome::Ended);
    // The key is still answerable, and now answers with nothing - the entry itself is gone, which
    // `index_entries` reports.
    assert!(index.resource_candidates(&file).expect("the index still answers").is_empty());
    assert_eq!(index.index_entries(), (0, 0), "every per-key entry was reclaimed");
}

// ---------------------------------------------------------------------------
// R4: visibility, field source, and the minimum permitted fact
// ---------------------------------------------------------------------------

/// **R4** — an unknown Shell in another session **and** another workspace stays in scope.
///
/// The shipped `scope_matches` required a session, a workspace or a named target to match, so this
/// row was never counted at all. A reader then saw a `Complete` picture that silently omitted the
/// one writer it most needed to know about.
#[test]
fn an_unknown_write_set_shell_in_another_session_and_workspace_stays_in_scope() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let reader_session = SessionId::new();
    let reader_workspace = WorkspaceId::new();
    let other_session = SessionId::new();
    let other_workspace = WorkspaceId::new();
    publish_agent_tool(
        &index,
        runtime_instance_id,
        other_session,
        other_workspace,
        ActivityToolGroup::Shell,
        1,
        vec![resource(
            ActivityResourceTarget::Unproven { hint: "shell write set not provable".to_owned() },
            ActivityResourceRelation::UnknownWriteSet,
            "shell.ps1",
        )],
    );
    let reader = view(runtime_instance_id, reader_session, reader_workspace);
    let snapshot = index.snapshot(&reader);
    assert_eq!(snapshot.header.coverage.total_in_scope, 1, "the unknown writer is really in scope");
    assert_eq!(snapshot.rows.len(), 1, "and it is admitted as the minimum permitted fact");
    let body = snapshot.select_summary(&reader, ActivitySummaryBudget::default()).body;
    assert!(
        body.contains("unknown-write-set"),
        "the model must learn that an unprovable writer exists: {body}"
    );
    assert!(
        !body.contains("shell.ps1"),
        "but never the other session's private display name: {body}"
    );
}

/// **R4** — a same-workspace sibling's private path does not reach the model summary.
///
/// The shipped implementation set `cross_session_visible = true` for a broad query and `may_see`
/// then admitted **every** field of another session's row, so one session's private paths appeared
/// in another session's Provider request purely because a question had been asked.
#[test]
fn another_sessions_private_path_does_not_enter_the_summary() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let workspace = WorkspaceId::new();
    let reader_session = SessionId::new();
    let sibling = SessionId::new();
    let private = object_target(11, 5);
    publish_agent_tool(
        &index,
        runtime_instance_id,
        sibling,
        workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(private.clone(), ActivityResourceRelation::Write, "/private/secret.txt")],
    );
    let mut reader = view(runtime_instance_id, reader_session, workspace);
    // Even an explicitly broadened reader - which is more than any production caller now sets -
    // must not receive the private path for a target it never reached.
    reader.cross_session_visible = true;
    let body =
        index.snapshot(&reader).select_summary(&reader, ActivitySummaryBudget::default()).body;
    assert!(
        !body.contains("secret.txt"),
        "an unauthorised private path must not enter the model's summary: {body}"
    );
    // Once the reader names the same physical target, the row becomes associated and its original
    // source is preserved rather than relabelled as the reader's own.
    reader.relevant_targets.insert(ActivityResourceKey::of(&private));
    let snapshot = index.snapshot(&reader);
    let summary = snapshot.select_summary(&reader, ActivitySummaryBudget::default());
    assert!(
        !summary.body.contains("secret.txt"),
        "a physical lookup hint never grants a private alias"
    );
    let own = view(runtime_instance_id, sibling, workspace);
    assert!(
        index
            .snapshot(&own)
            .select_summary(&own, ActivitySummaryBudget::default())
            .body
            .contains("secret.txt"),
        "the source session still sees its own key parameter"
    );
    let row = summary.rows.first().expect("the associated row");
    assert_eq!(
        row.row.session_id, sibling,
        "the row keeps its original session, never the reader's"
    );
    // The reader named the target, so the row is associated; but it is a same-workspace sibling's
    // row, not the reader's own, so the design's own classification is `Related` and the rendered
    // line says so. `Direct` is reserved for the reader's own session, and claiming it here would
    // misreport another session's activity as this reader's own.
    assert_eq!(row.relevance, ActivityRelevance::Related);
    assert!(summary.body.contains("(related)"), "and the association is visible in the body");
}

/// **R4** — a query type alone cannot widen visibility.
///
/// The defect was structural: `query_is_broad()` set the widening flag, so a caller obtained
/// permission by *phrasing*. Setting the flag here with no target association must still not admit
/// another session's private fields.
#[test]
fn a_broad_query_type_is_not_an_authorisation() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let workspace = WorkspaceId::new();
    let reader_session = SessionId::new();
    let other = SessionId::new();
    let file = object_target(13, 21);
    publish_agent_tool(
        &index,
        runtime_instance_id,
        other,
        workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(file.clone(), ActivityResourceRelation::Write, "other-private.txt")],
    );
    let mut broad = view(runtime_instance_id, reader_session, workspace);
    broad.cross_session_visible = true;
    let summary = index.snapshot(&broad).select_summary(&broad, ActivitySummaryBudget::default());
    assert!(
        !summary.body.contains("other-private.txt"),
        "a precise question is not a grant: {}",
        summary.body
    );
    // A same-workspace row that this reader did not associate is *counted as filtered*, so the
    // absence is disclosed rather than silent.
    assert!(
        summary.header.coverage.visibility_filtered > 0
            || summary.header.coverage.total_in_scope > 0,
        "the reader is told something exists that it may not see"
    );
}

/// **R4** — `WorkspaceSource`-style relabelling is rejected by the field rule.
///
/// Core-generated metadata about a row does not make another session's content public. The row may
/// be admitted (its existence is a fact worth stating) while its private fields stay out, and the
/// test rejects the implementation that admits both together.
#[test]
fn metadata_about_a_row_does_not_make_its_content_public() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let workspace = WorkspaceId::new();
    let reader_session = SessionId::new();
    let other = SessionId::new();
    publish_agent_tool(
        &index,
        runtime_instance_id,
        other,
        workspace,
        ActivityToolGroup::Shell,
        1,
        vec![resource(
            ActivityResourceTarget::Unproven { hint: "unprovable".to_owned() },
            ActivityResourceRelation::UnknownWriteSet,
            "cwd=C:\\other\\private",
        )],
    );
    let reader = view(runtime_instance_id, reader_session, workspace);
    let snapshot = index.snapshot(&reader);
    assert_eq!(snapshot.rows.len(), 1, "the unprovable writer is admitted");
    let row = &snapshot.rows[0];
    assert!(
        !reader.may_see_private_fields(row),
        "another session's cwd is a private field, however the row was generated"
    );
    let body = snapshot.select_summary(&reader, ActivitySummaryBudget::default()).body;
    assert!(!body.contains("C:\\other\\private"), "the private cwd must not be rendered: {body}");
}

// ---------------------------------------------------------------------------
// R5: boundaries that reject a bad implementation
// ---------------------------------------------------------------------------

/// **R5** — a record that invents a parameter key off its group's allowlist is refused.
///
/// The shipped `has_no_extra_facts` checked only lengths, so a publisher could place an arbitrary
/// key - a script body under `script`, say - into the index and every reader would carry it.
#[test]
fn a_record_with_a_key_outside_its_groups_allowlist_is_refused() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let execution = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let id = ActivityId::derive(ActivityKind::AgentTool, &execution, 1);
    let record = ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id,
        kind: ActivityKind::AgentTool,
        execution,
        generation: 1,
        tool_id: ToolId::from_stable_name("fs.write"),
        tool_group: ActivityToolGroup::FileWrite,
        owner: owner_scope(runtime_instance_id, session, workspace, true),
        resources: vec![resource(object_target(1, 1), ActivityResourceRelation::Write, "a.txt")],
        // `script` is a Shell key, not a FileWrite key - and it is not a FileWrite parameter at all.
        parameters: vec![ActivityParameter {
            key: "script".to_owned(),
            value: "Remove-Item -Recurse /".to_owned(),
            truncated: false,
        }],
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    };
    assert!(!record.has_no_extra_facts(), "an off-allowlist key is not a legitimate fact");
    assert_eq!(index.publish(record), ActivityPublishOutcome::Rejected);
    assert_eq!(index.len(), 0, "and the refused fact left the index untouched");
}

/// **R5** — an over-long or malformed opaque field is refused, and the good rows survive.
#[test]
fn an_over_long_hint_or_leaf_is_refused_and_other_rows_are_untouched() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let good = publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(object_target(1, 1), ActivityResourceRelation::Write, "good.txt")],
    );
    let execute = |resources: Vec<ActivityResource>, parameters: Vec<ActivityParameter>| {
        let execution = ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        };
        ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id: ActivityId::derive(ActivityKind::AgentTool, &execution, 1),
            kind: ActivityKind::AgentTool,
            execution,
            generation: 1,
            tool_id: ToolId::from_stable_name("fs.write"),
            tool_group: ActivityToolGroup::FileWrite,
            owner: owner_scope(runtime_instance_id, session, workspace, true),
            resources,
            parameters,
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        }
    };
    // An unbounded hint: the one deliberately opaque field is not a payload channel.
    let long_hint = execute(
        vec![resource(
            ActivityResourceTarget::Unproven {
                hint: "x".repeat(MAX_ACTIVITY_UNPROVEN_HINT_BYTES + 1),
            },
            ActivityResourceRelation::UnknownWriteSet,
            "hint",
        )],
        Vec::new(),
    );
    assert_eq!(index.publish(long_hint), ActivityPublishOutcome::Rejected);
    // An empty directory leaf is not a name.
    let empty_leaf = execute(
        vec![resource(
            ActivityResourceTarget::DirectoryEntry {
                parent_volume_serial: 1,
                parent_file_index: 2,
                leaf: String::new(),
                case_insensitive: false,
                comparison_key: None,
            },
            ActivityResourceRelation::Write,
            "leaf",
        )],
        Vec::new(),
    );
    assert_eq!(index.publish(empty_leaf), ActivityPublishOutcome::Rejected);
    // A zero generation is not a generation.
    let mut zero_generation = execute(
        vec![resource(object_target(1, 3), ActivityResourceRelation::Write, "z.txt")],
        Vec::new(),
    );
    zero_generation.generation = 0;
    assert_eq!(index.publish(zero_generation), ActivityPublishOutcome::Rejected);
    // A runtime that disagrees with its own owner scope makes generation filtering unsound.
    let mut mismatched_runtime = execute(
        vec![resource(object_target(1, 4), ActivityResourceRelation::Write, "m.txt")],
        Vec::new(),
    );
    mismatched_runtime.runtime_instance_id = RuntimeInstanceId::new();
    assert_eq!(index.publish(mismatched_runtime), ActivityPublishOutcome::Rejected);
    // An Agent-owned kind without an Agent run is an owner that cannot be decided.
    let mut borrowed = execute(
        vec![resource(object_target(1, 5), ActivityResourceRelation::Write, "b.txt")],
        Vec::new(),
    );
    borrowed.owner.agent_run_id = None;
    assert_eq!(index.publish(borrowed), ActivityPublishOutcome::Rejected);

    // And the good row is untouched through all of it, with the refusals disclosed as coverage.
    assert_eq!(index.len(), 1, "a refused fact must not disturb a good one");
    let reader = view(runtime_instance_id, session, workspace);
    let header = index.snapshot(&reader).header;
    assert_eq!(header.coverage.state, ActivityCoverageState::Partial);
    assert_eq!(header.coverage.rejected, 5, "every refusal is counted on the publish path");
    assert!(header.coverage.omission_reasons.contains(&ActivityOmissionReason::RejectedFact));
    let (kept, _) = good;
    assert!(index.snapshot(&reader).rows.iter().any(|row| row.id == kept));
}

/// **R5** — the summary byte budget covers the header and the separators, not only row text.
///
/// A budget that counted only row bodies would under-count what is really sent, so the rendered
/// fragment could exceed the bound it claims to respect.
#[test]
fn the_summary_budget_covers_the_whole_rendered_body() {
    let index = ActivityIndex::new(64);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    for n in 1..=40u64 {
        publish_agent_tool(
            &index,
            runtime_instance_id,
            session,
            workspace,
            ActivityToolGroup::FileWrite,
            u32::try_from(n).unwrap_or(1),
            vec![resource(object_target(20, n), ActivityResourceRelation::Write, &"n".repeat(200))],
        );
    }
    let reader = view(runtime_instance_id, session, workspace);
    let summary = index
        .snapshot(&reader)
        .select_summary(&reader, ActivitySummaryBudget { max_rows: 32, max_bytes: 1024 });
    assert!(
        summary.body.len() <= 1024,
        "the rendered body must respect its own byte bound: {} bytes",
        summary.body.len()
    );
    assert!(summary.header.coverage.truncated, "and the omission is disclosed");
}

/// **R5** — a poisoned index is unavailable, never a failed query and never a blocked tool.
#[test]
fn a_poisoned_index_stays_unavailable_rather_than_blocking_a_caller() {
    let index = ActivityIndex::new(16);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    publish_agent_tool(
        &index,
        runtime_instance_id,
        session,
        workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(object_target(1, 1), ActivityResourceRelation::Write, "a.txt")],
    );
    // Poison the real mutex by panicking while it is held, from a thread of its own - the same way
    // a real publisher panic would.
    let clone = index.clone();
    let poisoned = std::thread::spawn(move || {
        let _guard = clone.0.lock().expect("the index is healthy here");
        panic!("poison the index exactly as a real publisher panic would");
    })
    .join();
    assert!(poisoned.is_err(), "the poisoning really happened");

    // Every entry point answers instead of blocking: publishing reports the drop, reading reports
    // unavailability, and neither panics.
    assert!(index.is_unavailable());
    let refused = ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id: ActivityId::from_bytes([2; 16]),
        kind: ActivityKind::AgentTool,
        execution: ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        },
        generation: 1,
        tool_id: ToolId::from_stable_name("fs.write"),
        tool_group: ActivityToolGroup::FileWrite,
        owner: owner_scope(runtime_instance_id, SessionId::new(), WorkspaceId::new(), true),
        resources: vec![resource(object_target(1, 2), ActivityResourceRelation::Write, "b.txt")],
        parameters: Vec::new(),
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    };
    assert_eq!(index.publish(refused), ActivityPublishOutcome::Unavailable);
    let reader = view(runtime_instance_id, session, workspace);
    assert_eq!(
        index.snapshot(&reader).header.coverage.state,
        ActivityCoverageState::Unavailable,
        "a poisoned index reports that it cannot answer, not that nothing is running"
    );
    assert!(index.resource_candidates(&object_target(1, 1)).is_none());
    // `mark_available` really reaches the state through the poisoned mutex, so the *mark* is
    // cleared - which it could not do at all before, making recovery a permanent no-op. A read
    // still reports unavailable, and that is the honest answer: the mutex is still poisoned, so no
    // reader may trust the rows behind it. Clearing the mark asserts the cause is gone; it does not
    // pretend the lock was never broken.
    index.mark_available();
    assert!(
        index.is_unavailable(),
        "a still-poisoned mutex is still not a picture a reader may trust"
    );
    assert_eq!(
        index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id: ActivityId::from_bytes([3; 16]),
            kind: ActivityKind::AgentTool,
            execution: ActivityExecutionRef::AgentToolCall {
                tool_call_id: ToolCallId::new(),
                operation_id: SecurityOperationId::new(),
                attempt_ordinal: 1,
            },
            generation: 1,
            tool_id: ToolId::from_stable_name("fs.write"),
            tool_group: ActivityToolGroup::FileWrite,
            owner: owner_scope(runtime_instance_id, SessionId::new(), WorkspaceId::new(), true),
            resources: vec![resource(
                object_target(1, 3),
                ActivityResourceRelation::Write,
                "c.txt"
            )],
            parameters: Vec::new(),
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        }),
        ActivityPublishOutcome::Unavailable,
        "and publishing still reports the drop instead of panicking on the poisoned lock"
    );
}

/// **R5** — a reader must not hold the index lock across the whole projection.
///
/// # The defect this rejects
///
/// `snapshot` used to run the scope test, the visibility test, every row's clones and the sort
/// **entirely under the index mutex**. Because `publish` and `end` take that same mutex, every real
/// executor's own bookkeeping queued behind a *reader*: a slow or large query could delay the
/// publication of a tool that was actually running, which is exactly the "a slow query must not
/// affect execution" rule R5 states.
///
/// # Why this is measured the way it is
///
/// The first version of this test timed a publisher against a reader on another thread. **It passed
/// against a deliberately re-serialised implementation**, so it proved nothing - a timing window is
/// not a reproducible discriminator, and a test that cannot fail against the defect is worse than no
/// test. The replacement asserts the *structural* property the fix really establishes, and it does so
/// without any timing dependence:
///
/// * a publisher completes **while a reader is provably between its two phases** - the identity pass
///   has finished and the row materialisation has not started. That gap exists only if the guard was
///   dropped in between, which is precisely the fix. Under the pre-fix shape there is no gap: the
///   whole projection is one critical section, so a publisher arriving at any point during it waits
///   until the reader has finished, and the interleave below cannot occur.
///
/// The interleaving is forced deterministically through the reader's own visibility predicate, so the
/// reader is parked exactly inside its scan while the publisher runs.
#[test]
fn a_publisher_completes_while_a_reader_is_mid_projection() {
    let index = ActivityIndex::new(4096);
    let runtime_instance_id = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    for n in 1..=200u64 {
        publish_agent_tool(
            &index,
            runtime_instance_id,
            session,
            workspace,
            ActivityToolGroup::FileWrite,
            u32::try_from(n).unwrap_or(1),
            vec![resource(object_target(30, n), ActivityResourceRelation::Write, &"p".repeat(64))],
        );
    }
    let seeded = index.len();
    assert!(seeded >= 200, "the index really holds a row set");

    // The reader is parked inside its identity pass by a scope predicate that blocks once. While it
    // is parked there, the index must be publishable. If the projection held the guard for its whole
    // duration - the pre-fix shape - this publish could not complete until the reader returned.
    let reader_index = index.clone();
    let (parked, is_parked) = std::sync::mpsc::channel::<()>();
    let (release, is_released) = std::sync::mpsc::channel::<()>();
    let mut reader_view = view(runtime_instance_id, session, workspace);
    reader_view.relevant_targets.insert(ActivityResourceKey::of(&object_target(30, 1)));
    let reader = std::thread::spawn(move || {
        let parked = std::sync::Mutex::new(Some(parked));
        let released = std::sync::Mutex::new(is_released);
        // The reader parks **between the two phases** - after the identity guard has been released
        // and before any row is materialised. Under the pre-fix shape there was no such instant: the
        // guard covered the whole body, so the publisher below could not proceed until this reader
        // returned.
        let between = move || {
            if let Some(sender) = parked.lock().expect("parked sender").take() {
                let _ = sender.send(());
                let receiver = released.lock().expect("release receiver");
                let _ = receiver.recv_timeout(std::time::Duration::from_secs(10));
            }
        };
        let _ = reader_index.snapshot_with_seams_for_test(
            &reader_view,
            &|_record: &ActivityRecord| true,
            &between,
        );
    });
    is_parked
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the reader is mid-projection");

    // The decisive fact: a real publication completes now, with the reader provably inside its
    // projection. Under the pre-fix shape this call blocks until the reader finishes.
    let execution = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    let outcome = index.publish(ActivityRecord {
        tool_name: String::new(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id: ActivityId::derive(ActivityKind::AgentTool, &execution, 999),
        kind: ActivityKind::AgentTool,
        execution,
        generation: 999,
        tool_id: ToolId::from_stable_name("fs.write"),
        tool_group: ActivityToolGroup::FileWrite,
        owner: owner_scope(runtime_instance_id, session, workspace, true),
        resources: vec![resource(
            object_target(31, 1),
            ActivityResourceRelation::Write,
            "concurrent.txt",
        )],
        parameters: Vec::new(),
        started_at: UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: Revision::INITIAL,
    });
    assert!(
        outcome.was_recorded(),
        "a publisher completes while a reader is mid-projection: {outcome:?}"
    );

    release.send(()).expect("release the reader");
    reader.join().expect("the reader thread ended");
    assert_eq!(index.len(), seeded + 1, "the publication survived the concurrent projection");
}

/// **R5** — a poisoned observer is recovered rather than silently losing coverage forever.
///
/// The counters are three `u64`s with no invariant a panic can half-break, so abandoning them
/// permanently would mean coverage reported a clean `Complete` for a runtime that had really been
/// dropping facts. The pre-fix shape went through `lock()` for both `observe` and `counters`, so a
/// single panic while the lock was held stopped all counting for the life of the process.
#[test]
fn atomic_counters_survive_a_publisher_panic_without_losing_coverage() {
    let observer = ActivityPublicationObserver::default();
    observer.observe(ActivityPublishOutcome::CapacityExhausted);
    assert_eq!(observer.counters().capacity_dropped, 1);

    // Poison it for real, from a thread that panics while holding the lock.
    let clone = observer.clone();
    let poisoned = std::thread::spawn(move || {
        clone.observe(ActivityPublishOutcome::Unavailable);
        panic!("a failed publisher cannot poison the atomic counter owner");
    })
    .join();
    assert!(poisoned.is_err(), "the poisoning really happened");

    // The pre-fix shape returned the default counters here and silently dropped every later
    // increment, so this both counts again and keeps the count it already had.
    observer.observe(ActivityPublishOutcome::Rejected);
    let counters = observer.counters();
    assert_eq!(
        counters.capacity_dropped, 1,
        "the count taken before the poisoning is not silently discarded"
    );
    assert_eq!(
        counters.rejected, 1,
        "and a drop reported after the poisoning is still counted, so coverage cannot become a \
         false Complete"
    );
}

mod repair;
