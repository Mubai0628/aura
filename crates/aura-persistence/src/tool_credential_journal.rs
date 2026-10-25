//! Tool-owned Secret journal. No secret value or hash of a secret is persisted here.
use crate::{
    PersistenceCoordinator, PersistenceError,
    tool_credential::{credential_state, require_exact_slot, validate_session_scope},
    tool_entities::{encode_record, load_all_records, load_optional_record, sqlite_revision},
};
use aura_domain_common::{
    CommandInvocationId, CredentialId, CredentialRef, CredentialRevision, MutationJournalId,
    PrincipalId, Revision, UtcTimestamp,
};
use aura_tool_domain::{
    ToolCredentialAuthorityRecordV1, ToolCredentialChangeRequestV1, ToolCredentialCreateRequestV1,
    ToolCredentialKindV1, ToolCredentialManagementTargetV1, ToolCredentialPlacementPolicyV1,
    ToolCredentialScopeV1, ToolCredentialStateV1, ToolCredentialUsagePolicyV1,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

pub(crate) const TOOL_CREDENTIAL_JOURNAL_SCHEMA: &str = r"
CREATE TABLE tool_credential_mutations (
    journal_id TEXT PRIMARY KEY,
    command_invocation_id TEXT NOT NULL UNIQUE,
    credential_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('prepared','staged','committed','cleaned','aborted')),
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    record_hash_json TEXT NOT NULL CHECK(json_valid(record_hash_json)),
    updated_at TEXT NOT NULL
) STRICT;
CREATE UNIQUE INDEX tool_credential_mutation_active ON tool_credential_mutations(credential_id)
WHERE state IN ('prepared','staged','committed');
";

/// Read-only session ownership, supplied by the store that owns the sessions.
/// It must not create a second task/session record or confer tool permission.
pub trait ToolCredentialScopeSource: Send + Sync {
    fn session_belongs_to(
        &self,
        workspace: aura_domain_common::WorkspaceId,
        session: aura_domain_common::SessionId,
    ) -> Result<bool, PersistenceError>;
    /// The installed V2 package tool this derived instance names, if any. Read by
    /// the owner of the V2 store before any credential transaction starts.
    fn package_tool_target(
        &self,
        _instance: aura_domain_common::ToolInstanceId,
    ) -> Result<Option<PackageToolCredentialFacts>, PersistenceError> {
        Ok(None)
    }
}

/// An installed, trusted V2 package HTTP tool's credential slot. The tool has
/// no row in this store; its derived identities and declared slot come from Core.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageToolCredentialFacts {
    pub tool_id: aura_domain_common::ToolId,
    pub package_id: aura_domain_common::ToolPackageId,
    pub instance_revision: Revision,
    pub slot_key: aura_tool_contract::CredentialSlotKey,
    pub placement: aura_tool_domain::ToolCredentialPlacementV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolCredentialMutationIntentV1 {
    Create { request: ToolCredentialCreateRequestV1 },
    Rotate { request: ToolCredentialChangeRequestV1 },
    Clear { request: ToolCredentialChangeRequestV1 },
    UpdateOrigins { request: aura_tool_domain::ToolCredentialOriginsRequestV1 },
}
impl ToolCredentialMutationIntentV1 {
    pub fn target(&self) -> &ToolCredentialManagementTargetV1 {
        match self {
            Self::Create { request } => &request.target,
            Self::UpdateOrigins { request } => &request.target,
            Self::Rotate { request } | Self::Clear { request } => &request.target,
        }
    }
    pub fn writes_secret(&self) -> bool {
        matches!(self, Self::Create { .. } | Self::Rotate { .. })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialMutationJournalV1 {
    pub journal_id: MutationJournalId,
    pub command_invocation_id: CommandInvocationId,
    pub intent: ToolCredentialMutationIntentV1,
    pub record: ToolCredentialAuthorityRecordV1,
    pub expected_row_revision: Option<Revision>,
    pub new_ref: Option<CredentialRef>,
    pub old_ref: Option<CredentialRef>,
    pub state: String,
}

impl PersistenceCoordinator {
    pub fn refresh_managed_tool_credential_projections(
        &self,
        target: ToolCredentialManagementTargetV1,
    ) -> Result<Vec<aura_projection_contract::ProjectionChange>, PersistenceError> {
        self.verify_managed_credential_scope(&target)?;
        if self.package_target(&target)?.is_some() {
            // A V2 package tool has no P07 projection; its page re-reads the package.
            return Ok(Vec::new());
        }
        let runtime = self.runtime_instance_id();
        self.dispatch(move |c| {
            let tx = c.transaction()?;
            let (_, package) = validate_target(&tx, &target, None)?;
            let package = package.ok_or(PersistenceError::ToolEntityStateConflict)?;
            let drafts = crate::tool_package_reads::package_projection_drafts(&tx, package)?;
            let (changes, _) =
                crate::tool_projection::insert_tool_projections(&tx, runtime, drafts)?;
            tx.commit()?;
            Ok(changes)
        })
    }
    fn verify_managed_credential_scope(
        &self,
        target: &ToolCredentialManagementTargetV1,
    ) -> Result<(), PersistenceError> {
        let Some(session) = target.session_id else {
            return Ok(());
        };
        let workspace = target.workspace_id.ok_or(PersistenceError::CredentialBindingMismatch)?;
        let owner = self.tool_credential_scope_owner()?;
        match owner {
            Some(owner) if owner.session_belongs_to(workspace, session)? => Ok(()),
            Some(_) => Err(PersistenceError::CredentialBindingMismatch),
            None => self.with_reader(|c| validate_session_scope(c, Some(workspace), Some(session))),
        }
    }

    /// Package facts for a target whose instance is a V2 package tool, read before
    /// this store's transaction so no lock of the other store is held inside it.
    fn package_target(
        &self,
        target: &ToolCredentialManagementTargetV1,
    ) -> Result<Option<PackageToolCredentialFacts>, PersistenceError> {
        let Some(owner) = self.tool_credential_scope_owner()? else { return Ok(None) };
        Ok(owner.package_tool_target(target.tool_instance_id)?.filter(|facts| {
            facts.slot_key == target.credential_slot_key && facts.placement == target.placement
        }))
    }

    pub fn list_managed_tool_credentials_v1(
        &self,
        target: ToolCredentialManagementTargetV1,
    ) -> Result<Vec<ToolCredentialAuthorityRecordV1>, PersistenceError> {
        self.verify_managed_credential_scope(&target)?;
        let owner = self.current_tool_principal()?;
        let package = self.package_target(&target)?;
        self.with_reader(move |connection| {
            let (tool_id, package_id) = validate_target(connection, &target, package.as_ref())?;
            let mut records = load_all_records::<ToolCredentialAuthorityRecordV1>(connection,
                "SELECT record_json, record_hash_json FROM tool_credentials ORDER BY credential_id")?;
            let pending = load_all_records::<ToolCredentialMutationJournalV1>(connection,
                "SELECT record_json, record_hash_json FROM tool_credential_mutations WHERE state IN ('prepared','staged','committed') ORDER BY journal_id")?;
            // A prepared create has no authoritative credential row yet, but the user
            // must be able to see and explicitly recover its original intent.
            for journal in pending {
                if !records.iter().any(|r| r.credential_id == journal.record.credential_id) {
                    records.push(journal.record);
                }
            }
            Ok(records.into_iter().filter(|r| r.authenticated_owner == owner && r.scope_allows(target.workspace_id, target.session_id)
                && r.usage_policy.allows(package_id, tool_id) && r.placement_policy.allows(&target.placement)).collect())
        })
    }

    pub fn prepare_tool_credential_mutation_v1(
        &self,
        command: CommandInvocationId,
        intent: ToolCredentialMutationIntentV1,
    ) -> Result<ToolCredentialMutationJournalV1, PersistenceError> {
        self.verify_managed_credential_scope(intent.target())?;
        let owner = self.current_tool_principal()?;
        let package = self.package_target(intent.target())?;
        self.dispatch(move |connection| {
            let tx = connection.transaction()?;
            if let Some(existing) = load_optional_record::<ToolCredentialMutationJournalV1, _>(&tx,
                "SELECT record_json, record_hash_json FROM tool_credential_mutations WHERE command_invocation_id=?1", [command.to_string()])? {
                if existing.intent != intent || existing.record.authenticated_owner != owner { return Err(PersistenceError::CredentialBindingMismatch); }
                if !matches!(existing.state.as_str(), "cleaned" | "committed") {return Err(PersistenceError::CredentialJournalStateConflict);}
                return Ok(existing);
            }
            let target = intent.target();
            let (tool_id, package_id) = validate_target(&tx, target, package.as_ref())?;
            let now = UtcTimestamp::now();
            let (mut record, expected) = match &intent {
                ToolCredentialMutationIntentV1::Create { request } => {
                    match request.scope {
                        ToolCredentialScopeV1::User => (),
                        ToolCredentialScopeV1::Workspace {workspace_id} if target.workspace_id == Some(workspace_id) => (),
                        ToolCredentialScopeV1::Session {workspace_id,session_id} if target.workspace_id == Some(workspace_id) && target.session_id == Some(session_id) => (),
                        _ => return Err(PersistenceError::CredentialBindingMismatch),
                    }
                    (ToolCredentialAuthorityRecordV1 {
                        allowed_origins: aura_tool_domain::canonical_credential_origins(&request.allowed_origins).map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                        credential_id: CredentialId::new(), credential_revision: CredentialRevision::INITIAL,
                        authenticated_owner: owner, scope: request.scope, credential_kind: ToolCredentialKindV1::ApiKey, state: ToolCredentialStateV1::Configured,
                        placement_policy: ToolCredentialPlacementPolicyV1 {allowed_placements: vec![target.placement.clone()]},
                        usage_policy: ToolCredentialUsagePolicyV1::Explicit {tool_package_ids: vec![], tool_ids: vec![tool_id]},
                        display_name: request.display_name.clone(), row_revision: Revision::new(1), created_at: now, updated_at: now,
                    }, None)
                }
                ToolCredentialMutationIntentV1::Rotate { request } | ToolCredentialMutationIntentV1::Clear { request } => {
                    let old = read_authority(&tx, request.credential_id)?.ok_or(PersistenceError::CredentialRecordNotFound)?;
                    authorize_record(&old, owner, target, tool_id, package_id)?;
                    if old.row_revision != request.expected_row_revision { return Err(PersistenceError::CredentialRowRevisionConflict); }
                    let mut next = old.clone();
                    next.row_revision = Revision::new(old.row_revision.value().checked_add(1).ok_or(PersistenceError::ToolEntityIntegrity)?);
                    next.credential_revision = old.credential_revision.next().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
                    next.updated_at = now;
                    next.state = if intent.writes_secret() {ToolCredentialStateV1::Configured} else {ToolCredentialStateV1::Revoked};
                    (next, Some(old.row_revision))
                }
                ToolCredentialMutationIntentV1::UpdateOrigins { request } => {
                    let mut next = read_authority(&tx, request.credential_id)?.ok_or(PersistenceError::CredentialRecordNotFound)?;
                    authorize_record(&next, owner, target, tool_id, package_id)?;
                    if next.row_revision != request.expected_row_revision { return Err(PersistenceError::CredentialRowRevisionConflict); }
                    next.allowed_origins = aura_tool_domain::canonical_credential_origins(&request.allowed_origins).map_err(|_| PersistenceError::ToolEntityIntegrity)?;
                    next.row_revision = Revision::new(next.row_revision.value().checked_add(1).ok_or(PersistenceError::ToolEntityIntegrity)?);
                    (next, Some(request.expected_row_revision))
                }
            };
            record.updated_at = now;
            record.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM tool_credential_mutations WHERE credential_id=?1 AND state IN ('prepared','staged','committed'))", [record.credential_id.to_string()], |r| r.get(0))?;
            if active { return Err(PersistenceError::CredentialJournalStateConflict); }
            let journal_id = MutationJournalId::new();
            let namespace = blake3::hash(owner.to_string().as_bytes()).to_hex().to_string();
            let new_ref = intent.writes_secret().then(|| format!("wincred://aura/tool/{namespace}/{journal_id}").parse()).transpose().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            let old_ref = if matches!(intent, ToolCredentialMutationIntentV1::UpdateOrigins { .. }) { None } else { read_binding(&tx, record.credential_id)? };
            let journal = ToolCredentialMutationJournalV1 {journal_id, command_invocation_id: command, intent, record, expected_row_revision: expected, new_ref, old_ref, state: "prepared".into()};
            let (json, hash) = encode_record(&journal)?;
            tx.execute("INSERT INTO tool_credential_mutations(journal_id,command_invocation_id,credential_id,state,record_json,record_hash_json,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![journal_id.to_string(), command.to_string(), journal.record.credential_id.to_string(), journal.state, json, hash, now.to_string()])?;
            tx.commit()?;
            Ok(journal)
        })
    }

    pub fn incomplete_tool_credential_mutations_v1(
        &self,
    ) -> Result<Vec<ToolCredentialMutationJournalV1>, PersistenceError> {
        self.with_reader(|c| load_all_records(c, "SELECT record_json, record_hash_json FROM tool_credential_mutations WHERE state IN ('prepared','staged','committed') ORDER BY journal_id"))
    }

    pub fn transition_tool_credential_mutation_v1(
        &self,
        journal_id: MutationJournalId,
        expected: &str,
        next: &str,
    ) -> Result<ToolCredentialMutationJournalV1, PersistenceError> {
        let expected = expected.to_owned();
        let next = next.to_owned();
        self.dispatch(move |c| {
            let tx = c.transaction()?;
            let mut j = load_journal(&tx, journal_id)?;
            if j.state != expected
                || !matches!(
                    (expected.as_str(), next.as_str()),
                    ("prepared", "staged" | "aborted")
                        | ("staged", "aborted")
                        | ("committed", "cleaned")
                )
            {
                return Err(PersistenceError::CredentialJournalStateConflict);
            }
            j.state = next;
            store_journal(&tx, &j)?;
            tx.commit()?;
            Ok(j)
        })
    }

    /// The metadata and Secret binding become visible in one SQLite transaction.
    pub fn commit_tool_credential_mutation_v1(
        &self,
        journal_id: MutationJournalId,
    ) -> Result<ToolCredentialMutationJournalV1, PersistenceError> {
        let journal = self.with_reader(|c| load_journal(c, journal_id))?;
        self.verify_managed_credential_scope(journal.intent.target())?;
        let package_facts = self.package_target(journal.intent.target())?;
        self.dispatch(move |c| {
            let tx = c.transaction()?;
            let mut j = load_journal(&tx, journal_id)?;
            if j.state == "committed" || j.state == "cleaned" { return Ok(j); }
            if j.state != "staged" && !(j.state == "prepared" && !j.intent.writes_secret()) { return Err(PersistenceError::CredentialJournalStateConflict); }
            let current = read_authority(&tx, j.record.credential_id)?;
            if current.as_ref().map(|r| r.row_revision) != j.expected_row_revision {return Err(PersistenceError::CredentialRowRevisionConflict);}
            // A changed instance or scope cannot be waved through by interrupted recovery.
            let (tool_id, package) = validate_target(&tx, j.intent.target(), package_facts.as_ref())?;
            authorize_record(&j.record, j.record.authenticated_owner, j.intent.target(), tool_id, package)?;
            let (json, hash) = encode_record(&j.record)?;
            tx.execute("INSERT INTO tool_credentials(credential_id,credential_revision,authenticated_owner,state,row_revision,record_json,record_hash_json,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(credential_id) DO UPDATE SET credential_revision=excluded.credential_revision,state=excluded.state,row_revision=excluded.row_revision,record_json=excluded.record_json,record_hash_json=excluded.record_hash_json,updated_at=excluded.updated_at", params![j.record.credential_id.to_string(), sqlite_u64(j.record.credential_revision.value())?,j.record.authenticated_owner.to_string(),credential_state(&j.record.state),sqlite_revision(j.record.row_revision)?,json,hash,j.record.updated_at.to_string()])?;
            if let Some(reference) = &j.new_ref {
                tx.execute("INSERT INTO tool_credential_secret_bindings(credential_id,credential_revision,credential_ref,updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(credential_id) DO UPDATE SET credential_revision=excluded.credential_revision,credential_ref=excluded.credential_ref,updated_at=excluded.updated_at", params![j.record.credential_id.to_string(),sqlite_u64(j.record.credential_revision.value())?,reference.to_string(), j.record.updated_at.to_string()])?;
            } else if !matches!(j.intent, ToolCredentialMutationIntentV1::UpdateOrigins { .. }) {tx.execute("DELETE FROM tool_credential_secret_bindings WHERE credential_id=?1", [j.record.credential_id.to_string()])?;}
            tx.execute("UPDATE tool_credential_authority_state SET candidate_set_revision=candidate_set_revision+1,updated_at=?1 WHERE singleton=1", [j.record.updated_at.to_string()])?;
            j.state = "committed".into(); store_journal(&tx,&j)?; tx.commit()?; Ok(j)
        })
    }

    pub fn tool_credential_outcome_v1(
        &self,
        request: aura_tool_domain::ToolCredentialOutcomeRequestV1,
    ) -> Result<aura_tool_domain::ToolCredentialOutcomeV1, PersistenceError> {
        self.verify_managed_credential_scope(&request.target)?;
        use aura_tool_domain::{
            ToolCredentialMutationDispositionV1 as Disposition, ToolCredentialMutationResultV1,
            ToolCredentialOutcomeStateV1 as State, ToolCredentialOutcomeV1,
        };
        let owner = self.current_tool_principal()?;
        let package_facts = self.package_target(&request.target)?;
        self.with_reader(move |c| {
            let (tool, package) = validate_target(c, &request.target, package_facts.as_ref())?;
            let journal = load_optional_record::<ToolCredentialMutationJournalV1, _>(c,
                "SELECT record_json,record_hash_json FROM tool_credential_mutations WHERE command_invocation_id=?1",[request.command_invocation_id.to_string()])?;
            let Some(journal) = journal else { return Ok(ToolCredentialOutcomeV1 {command_invocation_id:request.command_invocation_id,state:State::NotObtained,result:None}); };
            authorize_record(&journal.record,owner,&request.target,tool,package)?;
            let actual = journal.intent.target();
            if actual.tool_instance_id != request.target.tool_instance_id || actual.credential_slot_key != request.target.credential_slot_key
                || actual.placement != request.target.placement || actual.workspace_id != request.target.workspace_id || actual.session_id != request.target.session_id {
                return Err(PersistenceError::CredentialBindingMismatch);
            }
            let (state, result) = match journal.state.as_str() {
                "committed" | "cleaned" => (State::Committed,Some(ToolCredentialMutationResultV1 {credential:Some(journal.record),disposition:Disposition::Committed,cleanup_pending:journal.state != "cleaned"})),
                "aborted" => (State::Aborted,Some(ToolCredentialMutationResultV1 {credential:None,disposition:Disposition::Aborted,cleanup_pending:false})),
                _ => (State::Pending,None),
            };
            Ok(ToolCredentialOutcomeV1 {command_invocation_id:request.command_invocation_id,state,result})
        })
    }

    pub fn authorized_tool_secret_binding_v1(
        &self,
        target: ToolCredentialManagementTargetV1,
        credential_id: CredentialId,
        revision: CredentialRevision,
    ) -> Result<(ToolCredentialAuthorityRecordV1, CredentialRef), PersistenceError> {
        self.verify_managed_credential_scope(&target)?;
        let owner = self.current_tool_principal()?;
        let package_facts = self.package_target(&target)?;
        self.with_reader(move |c| {
            let (tool_id, package) = validate_target(c, &target, package_facts.as_ref())?;
            let record = read_authority(c, credential_id)?.ok_or(PersistenceError::CredentialRecordNotFound)?;
            authorize_record(&record, owner, &target, tool_id, package)?;
            if record.state != ToolCredentialStateV1::Configured || record.credential_revision != revision { return Err(PersistenceError::CredentialUnavailable); }
            let pending: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM tool_credential_mutations WHERE credential_id=?1 AND state IN ('prepared','staged'))", [credential_id.to_string()], |r| r.get(0))?;
            if pending {return Err(PersistenceError::CredentialUnavailable);}
            let bound_revision: Option<i64> = c.query_row("SELECT credential_revision FROM tool_credential_secret_bindings WHERE credential_id=?1",[credential_id.to_string()],|r| r.get(0)).optional()?;
            if bound_revision != Some(sqlite_u64(revision.value())?) {return Err(PersistenceError::CredentialBindingMismatch);}
            Ok((record,read_binding(c,credential_id)?.ok_or(PersistenceError::CredentialUnavailable)?))
        })
    }
}

fn validate_target(
    c: &Connection,
    t: &ToolCredentialManagementTargetV1,
    package_tool: Option<&PackageToolCredentialFacts>,
) -> Result<(aura_domain_common::ToolId, Option<aura_domain_common::ToolPackageId>), PersistenceError>
{
    let row: Option<(i64,String,Option<String>)> = c.query_row("SELECT i.entity_revision,i.tool_id,d.tool_package_id FROM tool_instances i JOIN tool_definitions d ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json WHERE i.tool_instance_id=?1", [t.tool_instance_id.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((revision, tool, package)) = row else {
        // A V2 package tool: Core resolved its slot and placement from the installed
        // manifest; the revision the user saw must still be the installed one.
        let facts = package_tool.ok_or(PersistenceError::ToolEntityNotFound)?;
        if facts.instance_revision != t.expected_instance_revision {
            return Err(PersistenceError::ToolEntityRevisionConflict);
        }
        return Ok((facts.tool_id, Some(facts.package_id)));
    };
    if sqlite_revision(t.expected_instance_revision)? != revision {
        return Err(PersistenceError::ToolEntityRevisionConflict);
    }
    let definition: aura_tool_domain::ToolDefinitionRecordV1 = load_optional_record(c,"SELECT d.record_json,d.record_hash_json FROM tool_definitions d JOIN tool_instances i ON d.tool_id=i.tool_id AND d.tool_version_json=i.tool_version_json WHERE i.tool_instance_id=?1",[t.tool_instance_id.to_string()])?.ok_or(PersistenceError::ToolEntityNotFound)?;
    require_exact_slot(&definition.runtime, &t.credential_slot_key, &t.placement)?;
    Ok((tool.parse()?, package.map(|p| p.parse()).transpose()?))
}
fn authorize_record(
    r: &ToolCredentialAuthorityRecordV1,
    owner: PrincipalId,
    t: &ToolCredentialManagementTargetV1,
    tool: aura_domain_common::ToolId,
    package: Option<aura_domain_common::ToolPackageId>,
) -> Result<(), PersistenceError> {
    r.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if r.authenticated_owner != owner
        || !r.scope_allows(t.workspace_id, t.session_id)
        || !r.usage_policy.allows(package, tool)
        || !r.placement_policy.allows(&t.placement)
    {
        return Err(PersistenceError::CredentialBindingMismatch);
    }
    Ok(())
}
fn read_authority(
    c: &Connection,
    id: CredentialId,
) -> Result<Option<ToolCredentialAuthorityRecordV1>, PersistenceError> {
    load_optional_record(
        c,
        "SELECT record_json,record_hash_json FROM tool_credentials WHERE credential_id=?1",
        [id.to_string()],
    )
}
fn read_binding(
    c: &Connection,
    id: CredentialId,
) -> Result<Option<CredentialRef>, PersistenceError> {
    c.query_row(
        "SELECT credential_ref FROM tool_credential_secret_bindings WHERE credential_id=?1",
        [id.to_string()],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(|s| s.parse().map_err(|_| PersistenceError::ToolEntityIntegrity))
    .transpose()
}
fn load_journal(
    c: &Connection,
    id: MutationJournalId,
) -> Result<ToolCredentialMutationJournalV1, PersistenceError> {
    load_optional_record(
        c,
        "SELECT record_json,record_hash_json FROM tool_credential_mutations WHERE journal_id=?1",
        [id.to_string()],
    )?
    .ok_or(PersistenceError::CredentialJournalNotFound)
}
fn store_journal(
    tx: &Transaction<'_>,
    j: &ToolCredentialMutationJournalV1,
) -> Result<(), PersistenceError> {
    let (json, hash) = encode_record(j)?;
    tx.execute("UPDATE tool_credential_mutations SET state=?1,record_json=?2,record_hash_json=?3,updated_at=?4 WHERE journal_id=?5",params![j.state,json,hash,UtcTimestamp::now().to_string(),j.journal_id.to_string()])?;
    Ok(())
}
fn sqlite_u64(value: u64) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::ToolEntityIntegrity)
}
