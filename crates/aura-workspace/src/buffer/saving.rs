//! Editor save admission, publication and settlement.
use super::*;

impl BufferStore {
    /// Saves a buffer and returns the publication facts to the composing caller.
    #[allow(clippy::too_many_arguments)]
    pub fn save_with_publications(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
        expected_revision: Revision,
    ) -> crate::WorkspacePublicationOutcome<LogicalFileStatus> {
        crate::WorkspacePublicationOutcome::capture(|publications| {
            self.save_with_authority(
                resolver,
                session_id,
                path,
                reservation_id,
                reservation_epoch,
                expected_revision,
                SaveAuthority::Ordinary,
                publications,
            )
        })
    }

    /// Saves one session's buffer, as an explicit save action.
    ///
    /// The global `operation_gate` used to be held across the whole body, so a save in one
    /// workspace serialised every buffer operation in every other workspace *and* held the index
    /// lock across the disk read and the atomic write. The gate exists to keep the index
    /// consistent, not to serialise I/O, so it is taken in short sections with the disk work
    /// between them.
    ///
    /// # The order that matters
    ///
    /// The caller's confirmed flow is: prepare the candidate in the bounded short section, release
    /// the carrier, wait for **full** admission, then re-enter the carrier to verify the original
    /// permission/lifecycle/object/`expected` before the real write. The previous version did the
    /// disk write *before* `commit_save` checked anything, so a revoke, a reload or a lifecycle
    /// freeze that landed while the write was in flight was reported **after** the bytes had
    /// already reached the disk — the write landed and the caller was then told the save had not
    /// executed. That is the defect this ordering removes:
    ///
    /// 1. **carrier** — resolve the buffer key and snapshot the bytes, the expected digest and the
    ///    reservation, then release every lock;
    /// 2. **common commit** — take the one shared [`EligibilityOwner`]'s commit on the real object,
    ///    verifying the live eligibility, the workspace lifecycle and the buffer's own facts. A
    ///    refusal here costs no I/O at all;
    /// 3. **real write** — the conditional, handle-bound publication, while the commit is held, so
    ///    a second writer on the same object cannot interleave;
    /// 4. **settle** — re-verify the buffer is still the one the save was prepared from, publish the
    ///    new identity, and release the commit only after that.
    ///
    /// The commit is released on every path, including an error, because it is the *write* that
    /// must be protected and not the reporting of it. A save whose write failed took no resource
    /// beyond the instant it tried.
    pub fn save(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
        expected_revision: Revision,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        crate::WorkspacePublicationOutcome::capture(|publications| {
            self.save_with_authority(
                resolver,
                session_id,
                path,
                reservation_id,
                reservation_epoch,
                expected_revision,
                SaveAuthority::Ordinary,
                publications,
            )
        })
        .result
    }

    /// The same save, with the lifecycle's own explicit authority.
    ///
    /// The workspace lifecycle really does end every edit inside it, so its save is allowed to run
    /// **while frozen** — that is the precondition that distinguishes it from an ordinary mutation
    /// that must be refused during a transition. Everything else is identical: it takes the same
    /// common commit from the same owner, so its write is protected against a Main Fs write on the
    /// same object exactly as an ordinary save is.
    pub fn save_for_lifecycle(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
        expected_revision: Revision,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        crate::WorkspacePublicationOutcome::capture(|publications| {
            self.save_with_authority(
                resolver,
                session_id,
                path,
                reservation_id,
                reservation_epoch,
                expected_revision,
                SaveAuthority::Lifecycle,
                publications,
            )
        })
        .result
    }

    /// Saves one buffer under an explicit authority, with the caller's `expected` reaching the disk.
    ///
    /// The arguments are the caller's own facts and its authority, not a bundle this function could
    /// derive: a save that guessed any of them would be deciding on the caller's behalf.
    #[allow(clippy::too_many_arguments)]
    fn save_with_authority(
        &self,
        resolver: &SecurePathResolver,
        session_id: SessionId,
        path: &WorkspacePath,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
        expected_revision: Revision,
        authority: SaveAuthority,
        publications: &mut Vec<crate::BufferPublicationReceipt>,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        // Section 1: the short carrier. Everything needed to do the write is captured here, and
        // then every lock is released before any I/O happens.
        let carrier = self.prepare_save(
            resolver,
            session_id,
            path,
            reservation_id,
            reservation_epoch,
            expected_revision,
        )?;
        // Section 2: the workspace lifecycle and the real object are checked **before** the disk is
        // touched, and the commit taken from the one shared owner holds the object across the write.
        if authority == SaveAuthority::Ordinary
            && self.workspace_gate(resolver.workspace_id())? == Some(LifecycleBufferGate::Frozen)
        {
            self.record_save_failure(
                &carrier.key,
                reservation_id,
                reservation_epoch,
                "editor.save_lifecycle_frozen",
            );
            return Err(WorkspaceError::LifecycleTransitionInProgress);
        }
        let object = self.object_resource(resolver, path)?;
        // **The object *and* the naming slot are committed together.**
        //
        // A save replaces the file with a new object, so protecting only the old `FileIdentity`
        // leaves the *entry* free: another entry could take a claim on the name while this save is
        // between its commit and its identity migration, and the replacement would then have been
        // published into a slot somebody else owns.
        //
        // The slot is committed at `Write` and the presented long eligibility is the one this buffer
        // holds. The owner requires a presented claim to cover **every** resource in the set, so the
        // buffer's claim is taken on the entry as well as the object (see `BufferStore::open`), and
        // this commit therefore covers both legitimately rather than by relaxing that check.
        let slot = ResourceFacts::root_unlabelled(Arc::new(resolver.clone())).slot(path)?;
        let commit = match self.eligibility.commit(
            &CommitRequest::per_resource(vec![
                aura_workspace_domain::ResourceRequest::new(object, ResourceAccess::Write),
                aura_workspace_domain::ResourceRequest::new(slot, ResourceAccess::Write),
            ]),
            session_id,
            carrier.execution_id,
            carrier.eligibility.as_ref(),
        ) {
            Ok(commit) => commit,
            // **A live short commit is a wait; a stale version is not.**
            //
            // Waiting cannot change a stale identity or stale bytes, so those keep the original
            // `FileVersionConflict`. A *retryable* refusal that is not a long editor claim means
            // another execution is in this resource right now and will finish — the file may still
            // be exactly what this save decided about — so it is reported as its own typed busy
            // state and the caller's asynchronous owner may wait within its original budget and
            // re-enter the unchanged save. Nothing was written and no in-memory state moved.
            Err(refusal) if refusal.retryable && !self.eligibility.is_edit_refusal(&refusal) => {
                self.record_save_conflict(&carrier.key, reservation_id, reservation_epoch);
                return Err(WorkspaceError::FileResourceBusy {
                    path: path.clone(),
                    refusal: Box::new(refusal),
                });
            }
            Err(_) => {
                self.record_save_conflict(&carrier.key, reservation_id, reservation_epoch);
                return Err(WorkspaceError::FileVersionConflict { path: path.clone() });
            }
        };
        // Section 3: the real write, with the object committed.
        //
        // THE PUBLICATION OPENS **BEFORE** THE WRITE WINDOW, WITH FACTS ALREADY HELD.
        //
        // The identity published here is the one the save is replacing: `carrier.key.file_identity`
        // was verified by `prepare_save` from the store's own open, and the object/slot commit above
        // was derived from the resolver's verified chain. Nothing is re-opened to obtain it, and no
        // path is resolved to learn it - the previous shape called `resolve_existing` *after* the
        // write purely to get an identity for the publication, which both re-opened the target and
        // reported an already-finished save as `running`.
        //
        // The call is a plain closure over an in-memory index; its outcome is discarded, so a full
        // or unavailable index can neither lengthen nor fail the save. It happens after the commit
        // above (nothing is published for a save that A refused to admit) and before `write_save`
        // (the row really covers the write window).
        let fact =
            (carrier.key.workspace_id, carrier.key.session_id, carrier.revision, path.clone());
        let start_identity = carrier.key.file_identity.clone();
        let activity = BufferActivityScope {
            publisher: self.activity_publisher.clone(),
            workspace_id: fact.0,
            session_id: fact.1,
            reservation_id,
            buffer_revision: fact.2,
            identity: start_identity.clone(),
            path: fact.3.clone(),
            resources: commit.requests().to_vec(),
            phase: BufferCommitPhase::Terminated,
        };
        activity.publish(BufferCommitPhase::Start);
        let written = self.write_save(
            resolver,
            path,
            &carrier,
            reservation_id,
            reservation_epoch,
            publications,
        );
        // Section 4: settle. The commit is released only after the buffer's own facts have been
        // re-verified against the live index, so a concurrent edit, revoke or reload between the
        // sections is refused instead of being silently overwritten by this write — and a new
        // in-memory revision after the save leaves the buffer dirty rather than pretending the
        // save published content it never saw.
        //
        // The commit's own facts are copied out before the carrier is consumed, because the
        // publication must describe *this* commit - the real object it published and the real
        // buffer revision it was prepared from - rather than whatever the store holds afterwards.
        let settled = written.and_then(|new_identity| {
            self.commit_save(carrier, new_identity, reservation_id, reservation_epoch)
        });
        self.eligibility.release_commit(commit);
        // The write window is over, so the activity ends here - at the real settlement, not at
        // whatever moment a value happened to be dropped. The identity is the one the commit
        // produced when it settled, and the identity the save was replacing when it did not: both
        // are facts the store already holds, so ending costs no file I/O either.
        let mut activity = activity;
        activity.phase =
            if settled.is_ok() { BufferCommitPhase::Settled } else { BufferCommitPhase::Failed };
        drop(activity);
        settled
    }

    /// The disk half of a save, with the caller's original condition reaching the exclusive handle.
    #[allow(clippy::too_many_arguments)]
    fn write_save(
        &self,
        resolver: &SecurePathResolver,
        path: &WorkspacePath,
        carrier: &PreparedSave,
        reservation_id: EditorReservationId,
        reservation_epoch: Revision,
        publications: &mut Vec<crate::BufferPublicationReceipt>,
    ) -> Result<FileIdentity, WorkspaceError> {
        let (current, current_identity) =
            read_disk_with_identity(resolver, path).inspect_err(|_| {
                self.record_save_failure(
                    &carrier.key,
                    reservation_id,
                    reservation_epoch,
                    "editor.save_read_failed",
                );
            })?;
        if current_identity != carrier.key.file_identity
            || ContentHash::blake3(&current) != carrier.disk_hash
        {
            self.record_save_conflict(&carrier.key, reservation_id, reservation_epoch);
            return Err(WorkspaceError::FileVersionConflict { path: path.clone() });
        }
        super::native_save::atomic_save(
            resolver,
            path,
            &carrier.bytes,
            &carrier.disk_hash,
            &carrier.key.file_identity,
            publications,
        )
        .inspect_err(|_| {
            self.record_save_failure(
                &carrier.key,
                reservation_id,
                reservation_epoch,
                "editor.save_write_failed",
            );
        })
    }

    pub fn save_all_dirty_for_lifecycle(
        &self,
        resolver: &SecurePathResolver,
    ) -> Result<(), WorkspaceError> {
        self.save_all_dirty_for_lifecycle_with_publications(resolver).result
    }

    pub fn save_all_dirty_for_lifecycle_with_publications(
        &self,
        resolver: &SecurePathResolver,
    ) -> crate::WorkspacePublicationOutcome<()> {
        crate::WorkspacePublicationOutcome::capture(|publications| {
            if self.workspace_gate(resolver.workspace_id())? != Some(LifecycleBufferGate::Frozen) {
                return Err(WorkspaceError::LifecycleTransitionInProgress);
            }
            let candidates = self
                .buffers
                .read()
                .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
                .iter()
                .filter(|(key, record)| key.workspace_id == resolver.workspace_id() && record.dirty)
                .map(|(key, record)| {
                    Ok((
                        key.session_id,
                        record.path.clone(),
                        record.reservation_id.ok_or(WorkspaceError::BufferReadOnly)?,
                        record.reservation_epoch,
                        record.revision,
                    ))
                })
                .collect::<Result<Vec<_>, WorkspaceError>>()?;
            for (session_id, path, reservation_id, reservation_epoch, revision) in candidates {
                self.save_with_authority(
                    resolver,
                    session_id,
                    &path,
                    reservation_id,
                    reservation_epoch,
                    revision,
                    SaveAuthority::Lifecycle,
                    publications,
                )?;
            }
            let _operation = self
                .operation_gate
                .lock()
                .map_err(|_| WorkspaceError::LockPoisoned("editor buffer operation gate"))?;
            for (key, record) in self
                .buffers
                .write()
                .map_err(|_| WorkspaceError::LockPoisoned("editor buffers"))?
                .iter_mut()
                .filter(|(key, _)| key.workspace_id == resolver.workspace_id())
            {
                let _ = key;
                record.active_editing = false;
                record.ime_composition_active = false;
            }
            Ok(())
        })
    }
}
