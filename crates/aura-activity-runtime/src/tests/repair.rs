use super::*;

fn private_entry(name: &str) -> ActivityResourceTarget {
    ActivityResourceTarget::VerifiedEntry {
        parent: aura_domain_common::FileIdentity {
            volume_serial_number: "123".into(),
            file_id: "aa".repeat(16),
        },
        pending: vec![name.into()],
        case_insensitive: true,
        comparison_keys: Some(vec!["SECRET-FILE.TXT".into()]),
        ancestors: Vec::new(),
    }
}

#[test]
fn private_entry_exact_projection_uses_only_the_readers_known_spelling() {
    let index = ActivityIndex::new(10);
    let runtime = runtime();
    let producer = SessionId::new();
    let producer_workspace = WorkspaceId::new();
    let producer_spelling = "sEcReT-fiLe.txt";
    let known = private_entry("SECRET-FILE.TXT");
    publish_agent_tool(
        &index,
        runtime,
        producer,
        producer_workspace,
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(
            private_entry(producer_spelling),
            ActivityResourceRelation::Write,
            producer_spelling,
        )],
    );
    let mut reader = view(runtime, SessionId::new(), WorkspaceId::new());
    reader.relevant_targets.insert(ActivityResourceKey::of(&known));
    reader.relevant_resources.push(known.clone());
    let snapshot = index.snapshot_query(&reader, &ActivityQuery::Resource { target: known });
    assert_eq!(snapshot.rows.len(), 1);
    let wire = serde_json::to_string(&snapshot.rows).expect("serialize authorized query rows");
    assert!(
        !wire.contains(producer_spelling),
        "an exact key must not disclose the producer's private spelling"
    );
    assert!(wire.contains("SECRET-FILE.TXT"), "the reader may reuse its own already-known target");
    let own = index.snapshot(&view(runtime, producer, producer_workspace));
    assert!(
        serde_json::to_string(&own.rows)
            .expect("serialize the owner's rows")
            .contains(producer_spelling),
        "the original owner keeps its useful names"
    );
}

#[test]
fn private_entry_redaction_cannot_erase_a_related_directory_warning() {
    let index = ActivityIndex::new(10);
    let runtime = runtime();
    let producer = SessionId::new();
    let mut record = held_test_record(runtime, producer, WorkspaceId::new());
    let target = private_entry("sEcReT-fiLe.txt");
    let ActivityResourceTarget::VerifiedEntry { parent, .. } = &target else { unreachable!() };
    let directory = ActivityResourceTarget::VerifiedObject {
        identity: parent.clone(),
        ancestors: Vec::new(),
        subtree: true,
    };
    record.resources = vec![resource(target, ActivityResourceRelation::Write, "private-child")];
    let id = record.id;
    assert!(index.publish(record).was_recorded());
    let mut reader = view(runtime, SessionId::new(), WorkspaceId::new());
    reader.relevant_resources.push(directory.clone());
    reader.relevant_targets.insert(ActivityResourceKey::of(&directory));
    let snapshot = index.snapshot_query(&reader, &ActivityQuery::Resource { target: directory });
    assert_eq!(snapshot.rows.len(), 1);
    let wire = serde_json::to_string(&snapshot.rows).expect("serialize authorized directory rows");
    assert!(!wire.contains("private-child"));
    assert!(!wire.contains("sEcReT-fiLe"));
    let summary = snapshot.select_summary(&reader, ActivitySummaryBudget::default());
    assert_eq!(summary.rows.len(), 1, "redaction must not erase the real directory-related writer");
    assert_eq!(summary.rows[0].row.id, id);
    assert_eq!(summary.rows[0].relevance, ActivityRelevance::Related);
    assert_eq!(
        snapshot.reproject(&reader).rows.len(),
        1,
        "the same frozen facts retain their relationship after projection"
    );
    let unrelated = view(runtime, SessionId::new(), WorkspaceId::new());
    assert!(
        snapshot.reproject(&unrelated).rows.is_empty(),
        "capturing a row grants no new visibility"
    );
}

#[test]
fn a_snapshot_cannot_mix_new_rows_with_an_old_revision() {
    let index = ActivityIndex::new(10);
    let runtime = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let (first, _) = publish_agent_tool(
        &index,
        runtime,
        session,
        workspace,
        ActivityToolGroup::FileWrite,
        1,
        Vec::new(),
    );
    let before = index.revision();
    let snapshot =
        index.snapshot_with_seams_for_test(&view(runtime, session, workspace), &|_| true, &|| {
            index.end(first, 1);
            publish_agent_tool(
                &index,
                runtime,
                session,
                workspace,
                ActivityToolGroup::FileRead,
                2,
                Vec::new(),
            );
        });
    assert_eq!(snapshot.header.revision, before);
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].id, first);
    assert!(index.revision() > before);
}

#[test]
fn busy_real_index_does_not_block_publish_or_end_and_does_not_keep_a_ghost() {
    let index = ActivityIndex::new(10);
    let runtime = runtime();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let (id, _) = publish_agent_tool(
        &index,
        runtime,
        session,
        workspace,
        ActivityToolGroup::FileRead,
        1,
        Vec::new(),
    );
    let held = index.0.lock().expect("real publication state is available before the hold");
    let worker = index.clone();
    let (send, recv) = std::sync::mpsc::channel();
    let join = std::thread::spawn(move || {
        let outcome = worker.end(id, 1);
        let record = held_test_record(runtime, session, workspace);
        let publication = worker.publish(record);
        send.send((outcome, publication)).expect("test observer remains alive");
    });
    let (end, publish) = recv
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("the real held index cannot hold a tool worker");
    assert_eq!(end, ActivityEndOutcome::IndexUnavailable);
    assert_eq!(publish, ActivityPublishOutcome::Unavailable);
    drop(held);
    join.join().expect("publisher thread did not panic");
    let snapshot = index.snapshot(&view(runtime, session, workspace));
    assert!(snapshot.rows.is_empty(), "queued exact end is drained before exposing rows");
    assert_eq!(snapshot.header.coverage.unavailable_dropped, 1);
}

fn held_test_record(
    runtime: RuntimeInstanceId,
    session: SessionId,
    workspace: WorkspaceId,
) -> ActivityRecord {
    let execution = ActivityExecutionRef::AgentToolCall {
        tool_call_id: ToolCallId::new(),
        operation_id: SecurityOperationId::new(),
        attempt_ordinal: 1,
    };
    ActivityRecord {
        id: ActivityId::derive(ActivityKind::AgentTool, &execution, 1),
        kind: ActivityKind::AgentTool,
        execution,
        generation: 1,
        tool_id: ToolId::from_stable_name("fs.read"),
        tool_name: "fs.read".into(),
        private_source: ActivityPrivateSource::WorkspaceMetadata,
        tool_group: ActivityToolGroup::FileRead,
        owner: owner_scope(runtime, session, workspace, true),
        resources: Vec::new(),
        parameters: Vec::new(),
        started_at: UtcTimestamp::now(),
        runtime_instance_id: runtime,
        published_at_revision: Revision::INITIAL,
    }
}

#[test]
fn a_related_target_hint_is_not_permission_to_see_private_fields() {
    let index = ActivityIndex::new(10);
    let runtime = runtime();
    let own = SessionId::new();
    let other = SessionId::new();
    let workspace = WorkspaceId::new();
    let known = object_target(7, 1);
    let unrelated = object_target(7, 2);
    let mut record = held_test_record(runtime, other, workspace);
    record.resources = vec![
        resource(known.clone(), ActivityResourceRelation::Write, "private-path-A"),
        resource(unrelated, ActivityResourceRelation::Write, "private-path-B"),
    ];
    record.parameters = vec![ActivityParameter {
        key: "path".into(),
        value: "private-key-parameter".into(),
        truncated: false,
    }];
    record.tool_group = ActivityToolGroup::FileWrite;
    assert_eq!(index.publish(record), ActivityPublishOutcome::Recorded);
    let mut visibility = view(runtime, own, WorkspaceId::new());
    visibility.relevant_targets.insert(ActivityResourceKey::of(&known));
    let snapshot = index.snapshot(&visibility);
    assert_eq!(snapshot.rows.len(), 1);
    let row = &snapshot.rows[0];
    assert_eq!(row.resources.len(), 1);
    assert!(row.resources[0].display_name.is_empty());
    assert!(row.parameters.is_empty());
    let text = serde_json::to_string(row).expect("the projected activity row serializes");
    assert!(!text.contains("private-"));
}

#[test]
fn full_identity_and_ancestry_distinguish_aliases_and_directory_overlap() {
    use aura_domain_common::FileIdentity;
    let parent = FileIdentity {
        volume_serial_number: "18446744073709551614".into(),
        file_id: "0000000000000000aaaaaaaaaaaaaaaa".into(),
    };
    let child = FileIdentity {
        volume_serial_number: parent.volume_serial_number.clone(),
        file_id: "0000000000000000bbbbbbbbbbbbbbbb".into(),
    };
    let directory = ActivityResourceTarget::VerifiedObject {
        identity: parent.clone(),
        ancestors: Vec::new(),
        subtree: true,
    };
    let file = ActivityResourceTarget::VerifiedObject {
        identity: child.clone(),
        ancestors: vec![parent.clone()],
        subtree: false,
    };
    let unrelated = ActivityResourceTarget::VerifiedObject {
        identity: child,
        ancestors: Vec::new(),
        subtree: false,
    };
    assert_ne!(ActivityResourceKey::of(&directory), ActivityResourceKey::of(&file));
    assert!(targets_related(&directory, &file));
    assert!(!targets_related(&directory, &unrelated));
    let index = ActivityIndex::new(10);
    let runtime = runtime();
    let other = SessionId::new();
    publish_agent_tool(
        &index,
        runtime,
        other,
        WorkspaceId::new(),
        ActivityToolGroup::FileWrite,
        1,
        vec![resource(file, ActivityResourceRelation::Write, "secret")],
    );
    let mut visibility = view(runtime, SessionId::new(), WorkspaceId::new());
    visibility.relevant_resources.push(directory.clone());
    let snapshot =
        index.snapshot_query(&visibility, &ActivityQuery::Resource { target: directory });
    assert_eq!(snapshot.rows.len(), 1);
    assert!(snapshot.rows[0].resources[0].display_name.is_empty());
}

#[test]
fn comparison_facts_are_consumed_once_and_linguistic_expansion_is_never_assumed() {
    let parent = aura_domain_common::FileIdentity {
        volume_serial_number: "123".into(),
        file_id: "aa".repeat(16),
    };
    let target = |name: &str, key: &str| ActivityResourceTarget::VerifiedEntry {
        parent: parent.clone(),
        pending: vec![name.into()],
        case_insensitive: true,
        comparison_keys: Some(vec![key.into()]),
        ancestors: Vec::new(),
    };
    let sharp = target("ß.txt", "ß.TXT");
    let ss = target("ss.txt", "SS.TXT");
    assert_ne!(ActivityResourceKey::of(&sharp), ActivityResourceKey::of(&ss));
    assert!(!targets_related(&sharp, &ss));
    assert_eq!(ActivityResourceKey::of(&ss), ActivityResourceKey::of(&target("SS.txt", "SS.TXT")));
    let mut missing = sharp.clone();
    let ActivityResourceTarget::VerifiedEntry { comparison_keys, .. } = &mut missing else {
        unreachable!()
    };
    *comparison_keys = None;
    assert!(matches!(ActivityResourceKey::of(&missing), ActivityResourceKey::Unproven { .. }));
    assert!(
        !targets_related(&missing, &missing),
        "two unknown keys are never proof of a shared target"
    );
}
