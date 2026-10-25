//! Ancestor metadata only, never directory enumeration or
//! inherited data rights. Durable intents/restoration remain in FileLease.
use super::*;
use crate::AncestorWork;
use aura_windows_platform::ExecutionMetadataAccess;

/// What one ancestor-metadata pass did and how it ended. Same contract as
/// [`GrantOutcome`]: the counts are real even when a control stopped the pass,
/// and a stop is never a finished grant.
pub struct AncestorOutcome {
    pub work: AncestorWork,
    pub verdict: GrantVerdict,
    pub failure: Option<anyhow::Error>,
}

impl AncestorOutcome {
    /// Fails unless the pass completed: a stop is an error, never a silent success.
    pub fn finish(self) -> anyhow::Result<AncestorWork> {
        match self.verdict {
            GrantVerdict::Complete => Ok(self.work),
            GrantVerdict::Stopped(reason) => {
                Err(anyhow::anyhow!("runner.ancestor_stopped.{}", reason.code()))
            }
            GrantVerdict::Failed => {
                Err(self.failure.unwrap_or_else(|| anyhow::anyhow!("runner.ancestor_failed")))
            }
        }
    }
}

impl FileLease {
    pub fn grant_ancestor_metadata(
        &mut self,
        root: &ExecutionFileBinding,
        access: &ExecutionMetadataAccess,
        read_roots: &[ExecutionFileBinding],
        control: &GrantControl,
    ) -> AncestorOutcome {
        let records_before = self.records_written;
        let mut work = AncestorWork::default();
        let mut failure = None;
        let mut stopped = None;
        {
            let mut step = || -> Result<()> {
                let retained = super::super::file_binding::verify(root)?;
                ensure!(retained.info().is_directory, "runner.scope_not_directory");
                let canonical = retained.info().final_path.clone();
                let readable = read_roots
                    .iter()
                    .map(super::super::file_binding::verify)
                    .collect::<Result<Vec<_>>>()?;
                let mut ancestors = canonical.ancestors().skip(1).collect::<Vec<_>>();
                ensure!(ancestors.len() <= 256, "runner.ancestor_limit");
                // First pin/check the complete chain. No mutation follows an unchecked
                // reparse point; all ancestors are held against rename until release.
                ancestors.reverse();
                let mut plans = Vec::new();
                for path in ancestors {
                    if let Some(reason) = control.stop_reason() {
                        stopped = Some(reason);
                        return Ok(());
                    }
                    let (pin, allowed) = access.pin_directory(path)?;
                    work.ancestors_pinned = work.ancestors_pinned.saturating_add(1);
                    ensure!(
                        canonical.starts_with(&pin.info().final_path),
                        "runner.ancestor_path_changed"
                    );
                    let granted_by_tree = readable
                        .iter()
                        .any(|r| aura_windows_platform::final_path_within(r.info(), pin.info()));
                    if !allowed && !granted_by_tree {
                        ensure!(
                            self.held.len() + plans.len() < MAX_OBJECTS,
                            "runner.scope_object_limit"
                        );
                        let object = Arc::new(ExecutionFileSecurity::open(path, true)?);
                        ensure!(
                            object.info().identity == pin.info().identity,
                            "runner.ancestor_changed"
                        );
                        let before = object.capture()?;
                        let after = object.plan(&[(access.account(), 0x80)])?;
                        ensure!(
                            access.allows_plan(&pin, &after)?,
                            "runner.ancestor_metadata_denied"
                        );
                        plans.push((object, before, after));
                    }
                    self.ancestor_pins.push(pin);
                }
                let apply_stopped = self.apply_ancestor_plans(plans, control, &mut work)?;
                stopped = apply_stopped;
                Ok(())
            };
            if let Err(source) = step() {
                failure = Some(source);
            }
        }
        // Durable records settle from the journal itself: a failed apply must
        // not lose the count of what the pass already persisted.
        work.journal_records = self.records_written.saturating_sub(records_before);
        let verdict = match (&failure, stopped) {
            (Some(_), _) => GrantVerdict::Failed,
            (None, Some(reason)) => GrantVerdict::Stopped(reason),
            (None, None) => GrantVerdict::Complete,
        };
        AncestorOutcome { work, verdict, failure }
    }

    fn apply_ancestor_plans(
        &mut self,
        plans: Vec<(Arc<ExecutionFileSecurity>, String, String)>,
        control: &GrantControl,
        work: &mut AncestorWork,
    ) -> Result<Option<GrantStop>> {
        let mut stopped = None;
        for (object, before, after) in plans {
            if let Some(reason) = control.stop_reason() {
                stopped = Some(reason);
                break;
            }
            self.record(&Record::Intent {
                path: object.info().final_path.clone(),
                volume: object.info().identity.volume_serial_number,
                id: object.info().identity.file_id,
                before: before.clone(),
                after: after.clone(),
            })?;
            self.held.push(Held { object, before, after });
            let held = self.held.last().expect("journaled ancestor");
            held.object.apply_exact(&held.before, &held.after)?;
            // Count at the success point: a failure on the next ancestor must
            // not lose the rewrites this pass already applied.
            work.acl_rewrites = work.acl_rewrites.saturating_add(1);
        }
        Ok(stopped)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    const ACCOUNT: &str = "S-1-5-21-333333-444444-555555-666666";

    #[test]
    fn metadata_lease_recovers_without_changing_children_or_file_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("private");
        std::fs::create_dir(&parent).unwrap();
        let child = parent.join("sibling.txt");
        std::fs::write(&child, "preserve this content").unwrap();
        let object = Arc::new(ExecutionFileSecurity::open(&parent, true).unwrap());
        let child = ExecutionFileSecurity::open(&child, false).unwrap();
        let child_before = child.capture().unwrap();
        let before = object.capture().unwrap();
        let after = object.plan(&[(ACCOUNT, 0x80)]).unwrap();
        assert!(
            after.contains(&format!("(A;;LO;;;{ACCOUNT})"))
                || after.contains(&format!("(A;;0x80;;;{ACCOUNT})")),
            "{after}"
        );
        let journal = tmp.path().join("lease.jsonl");
        let mut lease = FileLease::create(&journal).unwrap();
        lease.bind_process("synthetic", "synthetic", "synthetic").unwrap();
        let mut work = AncestorWork::default();
        let stopped = lease
            .apply_ancestor_plans(
                vec![(object.clone(), before.clone(), after)],
                &GrantControl::unbounded(),
                &mut work,
            )
            .unwrap();
        assert_eq!(work.acl_rewrites, 1);
        assert!(stopped.is_none());
        assert_eq!(child.capture().unwrap(), child_before);
        drop(lease); // Bound lease leaves a durable intent for observed-stop recovery.
        super::super::recover(&journal).unwrap();
        assert!(ExecutionFileSecurity::sddl_equivalent(&object.capture().unwrap(), &before));
        assert_eq!(
            std::fs::read_to_string(parent.join("sibling.txt")).unwrap(),
            "preserve this content"
        );
    }

    #[test]
    fn metadata_lease_preserves_foreign_acl_and_restores_stacked_leases_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let object = Arc::new(ExecutionFileSecurity::open(tmp.path(), true).unwrap());
        let before = object.capture().unwrap();
        let after = object.plan(&[(ACCOUNT, 0x80)]).unwrap();
        let mut first = FileLease::create(&tmp.path().join("one.jsonl")).unwrap();
        let mut work = AncestorWork::default();
        let stopped = first
            .apply_ancestor_plans(
                vec![(object.clone(), before.clone(), after.clone())],
                &GrantControl::unbounded(),
                &mut work,
            )
            .unwrap();
        assert_eq!(work.acl_rewrites, 1);
        assert!(stopped.is_none());
        let second_sid = "S-1-5-21-333333-444444-555555-777777";
        let second_after = object.plan(&[(second_sid, 0x80)]).unwrap();
        let mut second = FileLease::create(&tmp.path().join("two.jsonl")).unwrap();
        let mut work = AncestorWork::default();
        let stopped = second
            .apply_ancestor_plans(
                vec![(object.clone(), after.clone(), second_after.clone())],
                &GrantControl::unbounded(),
                &mut work,
            )
            .unwrap();
        assert_eq!(work.acl_rewrites, 1);
        assert!(stopped.is_none());
        assert!(first.restore_existing().is_err()); // Never overwrite a newer/foreign DACL.
        assert!(ExecutionFileSecurity::sddl_equivalent(&object.capture().unwrap(), &second_after));
        second.restore().unwrap();
        first.restore().unwrap();
        assert!(ExecutionFileSecurity::sddl_equivalent(&object.capture().unwrap(), &before));
    }

    /// A failure on the second plan must not lose the first plan's rewrite:
    /// the count lands where the apply succeeds, not in a batch after the loop.
    #[test]
    fn a_failed_second_plan_keeps_the_first_rewrite_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let one = tmp.path().join("one");
        let two = tmp.path().join("two");
        std::fs::create_dir(&one).unwrap();
        std::fs::create_dir(&two).unwrap();
        let first = Arc::new(ExecutionFileSecurity::open(&one, true).unwrap());
        let second = Arc::new(ExecutionFileSecurity::open(&two, true).unwrap());
        let before_one = first.capture().unwrap();
        let after_one = first.plan(&[(ACCOUNT, 0x80)]).unwrap();
        let before_two = second.capture().unwrap();
        let after_two = second.plan(&[(ACCOUNT, 0x80)]).unwrap();
        // The second object's DACL moves after its plan was captured, so its
        // apply refuses instead of overwriting a foreign change.
        let foreign_sid = "S-1-5-21-333333-444444-555555-777777";
        let foreign = second.plan(&[(foreign_sid, 0x80)]).unwrap();
        second.apply_exact(&before_two, &foreign).unwrap();
        let journal = tmp.path().join("lease.jsonl");
        let mut lease = FileLease::create(&journal).unwrap();
        let mut work = AncestorWork::default();
        let error = lease
            .apply_ancestor_plans(
                vec![
                    (first.clone(), before_one.clone(), after_one),
                    (second.clone(), before_two.clone(), after_two.clone()),
                ],
                &GrantControl::unbounded(),
                &mut work,
            )
            .expect_err("the second apply refuses the foreign DACL");
        assert_eq!(format!("{error:#}"), "execution ACL changed; preserve journal");
        assert_eq!(work.acl_rewrites, 1, "the first rewrite survives the failure");
        // Undo the foreign change, then the lease restores what it applied.
        second.apply_exact(&foreign, &before_two).unwrap();
        lease.restore().unwrap();
        assert!(ExecutionFileSecurity::sddl_equivalent(&first.capture().unwrap(), &before_one));
        assert!(ExecutionFileSecurity::sddl_equivalent(&second.capture().unwrap(), &before_two));
    }
}
