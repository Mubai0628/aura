//! Reconcile temporary access only after the actual previous process boundary
//! is stopped. No SQL, result fabrication, command replay or elevation.
use anyhow::{Result, ensure};
use aura_domain_common::ContentHash;
use aura_windows_platform::RootJob;
use aura_windows_user_contract::ExecutionFileBinding;
use std::path::Path;

mod released_index;

fn canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()
            }
        })
}

pub fn recover_idle(root: &Path, installation: &str, owner: &str) -> Result<serde_json::Value> {
    let _setup = aura_windows_platform::lock_execution_resources(
        &root.join("private/execution-setup.lock"),
    )?;
    recover_for_setup(root, installation, owner)
}

/// Setup already holds its separate exclusion in the parent process. This
/// lock still excludes every Core dispatch; recorded Jobs supply stop proof.
pub fn recover_for_setup(
    root: &Path,
    installation: &str,
    owner: &str,
) -> Result<serde_json::Value> {
    let _execution =
        aura_windows_platform::lock_execution_resources(&root.join("private/execution.lock"))?;
    Ok(recover_locked(root, installation, owner)?.0)
}

/// Every fact one journal contributes to a recovery pass, from either the
/// trusted released index or one full read of the journal itself.
struct JournalFacts {
    order: u64,
    boundary: Option<(String, String, String)>,
    roots: Vec<ExecutionFileBinding>,
    sha256: ContentHash,
    released: bool,
}

/// Caller holds both fixed exclusion files; no other Core/Setup can launch.
///
/// Returns the setup receipt together with the measured work, so the runtime
/// opening this block can attach the real cost to the launch that paid it.
pub(crate) fn recover_locked(
    root: &Path,
    installation: &str,
    owner: &str,
) -> Result<(serde_json::Value, crate::work::RecoveryWork)> {
    let started = std::time::Instant::now();
    let mut work = crate::work::RecoveryWork::default();
    ensure!(
        aura_windows_platform::current_user_token_identity()?.0 == owner,
        "runner.recovery_owner"
    );
    for relative in ["private", "private/execution-leases"] {
        aura_windows_platform::verify_execution_installation_acl(
            &root.join(relative),
            owner,
            true,
        )?;
    }
    let journal_root = root.join("private/execution-leases");
    let (_, _journals) = crate::file_binding::capture(&journal_root, false)?;
    let mut paths = std::fs::read_dir(&journal_root)?
        .map(|entry| Ok(entry?.path()))
        .collect::<Result<Vec<_>>>()?;
    paths.sort();
    let boot = aura_windows_platform::execution_maintenance::boot_identity()?;

    let mut listed = Vec::new();
    for path in &paths {
        ensure!(path.extension().is_some_and(|v| v == "jsonl"), "runner.foreign_lease_file");
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| anyhow::anyhow!("runner.foreign_lease_file"))?;
        listed.push((path.clone(), name.to_owned()));
    }
    let mut index = released_index::ReleasedIndex::load(root, installation);
    index.retain(&listed.iter().map(|(_, name)| name.clone()).collect());

    // Classification pass. An index hit is only believed while the journal still
    // is the file the entry was built from: the same object, the same size, and
    // the same change generation, read through a handle that excludes writers —
    // so a rewrite still in progress refuses the check instead of passing it.
    // Before any entry is trusted, the volume's change counter is shown to have
    // moved over a write this pass just made; a counter that cannot move cannot
    // have described anyone else's write either. Anything that does not match
    // exactly takes the full read below.
    let epoch_live = if index.is_empty() {
        false
    } else {
        match aura_windows_platform::stamp_change_epoch(&change_epoch_path(root)) {
            Ok(current) => {
                let live = aura_windows_platform::epoch_advanced(index.epoch(), current);
                index.set_epoch(current);
                live
            }
            Err(_) => false,
        }
    };
    let mut facts = Vec::new();
    for (path, name) in listed {
        work.journals += 1;
        let cached = if epoch_live {
            index.entry(&name).filter(|entry| {
                work.witnessed += 1;
                entry.matches(&path)
            })
        } else {
            None
        };
        let fact = if let Some(entry) = cached {
            work.indexed += 1;
            JournalFacts {
                order: 0,
                boundary: None,
                roots: entry.roots.clone(),
                sha256: entry.sha256.clone(),
                released: true,
            }
        } else {
            scan_and_index(&path, &name, &mut index, &mut work)?
        };
        facts.push((path, name, fact));
    }

    // Validate every stop boundary before applying the first restoration.
    let mut held_jobs = Vec::new();
    for (path, _name, fact) in &facts {
        if fact.released {
            continue;
        }
        let (bound_installation, nonce, old_boot) = fact
            .boundary
            .clone()
            .ok_or_else(|| anyhow::anyhow!("runner.legacy_lease_stop_unproven"))?;
        ensure!(
            bound_installation == installation
                && path.file_stem().and_then(|v| v.to_str()) == Some(nonce.as_str()),
            "runner.lease_process_binding_changed"
        );
        ensure!(
            canonical_uuid(&old_boot)
                && nonce.len() == 32
                && nonce.bytes().all(|v| v.is_ascii_hexdigit() && !v.is_ascii_uppercase()),
            "runner.lease_boot_identity_invalid"
        );
        if old_boot == boot
            && let Some(job) = RootJob::open_for_recovery(installation, &nonce)?
        {
            // Taking a new handle during old-Core handle cleanup can
            // delay kill-on-close, so terminate and observe explicitly.
            job.stop_for_recovery()?;
            held_jobs.push(job);
        }
        // Different boot: the old kernel Job/process objects no longer exist.
    }

    // Restore overlapping runtime leases in their durable application order.
    // Released history is not re-read here: it has nothing left to restore.
    let mut pending: Vec<&(std::path::PathBuf, String, JournalFacts)> =
        facts.iter().filter(|(_, _, fact)| !fact.released).collect();
    pending.sort_by_key(|(_, _, fact)| std::cmp::Reverse(fact.order));
    let pending_paths = pending.iter().map(|(path, _, _)| path.clone()).collect::<Vec<_>>();
    crate::file_lease::recover_all(&pending_paths)?;
    // A journal released by the restore above changed content (the durable
    // ObjectsRestored/Released records), so its final facts come from one
    // fresh read, which also indexes it for the next open.
    for (path, name, fact) in facts.iter_mut().filter(|(_, _, fact)| !fact.released) {
        *fact = scan_and_index(path, name, &mut index, &mut work)?;
        ensure!(fact.released, "runner.lease_not_released");
    }

    let scratch_parent = root.join("execution-scratch");
    let parent = aura_windows_platform::open_verified_publish_parent(&scratch_parent)?;
    let mut restored = Vec::new();
    for (_path, name, fact) in &facts {
        for binding in &fact.roots {
            let candidate = Path::new(&binding.path);
            if candidate.parent() != Some(parent.info().final_path.as_path()) {
                continue;
            }
            let root_name = candidate
                .file_name()
                .and_then(|v| v.to_str())
                .ok_or_else(|| anyhow::anyhow!("runner.scratch_name"))?;
            ensure!(
                root_name.len() == 32
                    && root_name.bytes().all(|v| v.is_ascii_hexdigit())
                    && name.strip_suffix(".jsonl") == Some(root_name),
                "runner.scratch_journal_mismatch"
            );
            if !candidate.exists() {
                continue;
            }
            let object = crate::file_binding::verify(binding)?;
            ensure!(
                object.info().is_directory
                    && aura_windows_platform::final_path_within(parent.info(), object.info()),
                "runner.scratch_escape"
            );
            drop(object);
            // Exact retired per-attempt root, verified above. Windows removal
            // does not follow a directory link. Workspace contents are retained.
            std::fs::remove_dir_all(candidate)?;
        }
        restored.push(serde_json::json!({"journal":name,"sha256":fact.sha256}));
    }
    ensure!(
        std::fs::read_dir(&scratch_parent)?.next().is_none(),
        "runner.unjournaled_scratch_preserved"
    );
    drop(held_jobs);
    work.ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    index.store(root)?;
    Ok((
        serde_json::json!({"schema":"windows-user-lease-recovery-v1","boot":boot,"restored":restored,"command_replayed":false,"work":serde_json::to_value(work)?}),
        work,
    ))
}

/// Fully read one journal, cache it when it has settled, and return its facts.
fn scan_and_index(
    path: &Path,
    name: &str,
    index: &mut released_index::ReleasedIndex,
    work: &mut crate::work::RecoveryWork,
) -> Result<JournalFacts> {
    let scan = crate::file_lease::scan_journal(path)?;
    work.parsed += 1;
    work.bytes += scan.size;
    work.records += scan.records;
    let sha256 = scan
        .binding
        .sha256
        .clone()
        .ok_or_else(|| anyhow::anyhow!("runner.lease_journal_invalid"))?;
    // Only a journal whose content and change generation were read through the
    // same writer-excluding handle can be indexed: the entry is a claim about the
    // state those two facts describe together, and a journal the volume would not
    // describe is simply read in full every time.
    if scan.released
        && let Some(change_usn) = scan.change_usn
    {
        index.insert(
            name.to_owned(),
            released_index::ReleasedEntry {
                volume_serial: scan.binding.volume_serial.clone(),
                file_id: scan.binding.file_id,
                size: scan.size,
                sha256: sha256.clone(),
                change_usn,
                roots: scan.roots.clone(),
            },
        );
    }
    Ok(JournalFacts {
        order: scan.order,
        boundary: scan.boundary,
        roots: scan.roots,
        sha256,
        released: scan.released,
    })
}

/// The volume liveness anchor's own file, beside the index it protects.
fn change_epoch_path(root: &Path) -> std::path::PathBuf {
    root.join("private/execution-leases-index-epoch.bin")
}
