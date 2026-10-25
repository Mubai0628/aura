//! Runtime closed loop for passes cancelled mid-enumeration. The halted-scope
//! fact proves no Execute followed, so the same runtime instance settles the
//! unit, removes the attempt scratch and accepts new read scopes without a
//! background conflict. An external restore fault or a legacy journal still
//! retains the runtime conservatively.
#![allow(clippy::unwrap_used)]
use crate::{
    file_lease::{FileLease, GrantControl, GrantSignal},
    runtime::RuntimeResources,
    supervisor::LaunchFacts,
};
use aura_windows_user_contract::{ExecutionAccount, ExecutionFileBinding};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const WRITE: &str = "S-1-5-21-333333-444444-555555-666666";

/// 32 lowercase hex, the shape recovery requires of a nonce; `seed` keeps
/// parallel tests on distinct kernel Job names.
fn nonce(seed: u8) -> String {
    format!("{seed:02x}").repeat(16)
}

fn acl(path: &Path) -> String {
    aura_windows_platform::ExecutionFileSecurity::open(path, false).unwrap().capture().unwrap()
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut map = BTreeMap::new();
    map.insert(root.to_path_buf(), acl(root));
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        map.insert(path.clone(), acl(&path));
    }
    map
}

fn assert_restored(snapshot: &BTreeMap<PathBuf, String>) {
    for (path, original) in snapshot {
        let current = acl(path);
        assert!(
            aura_windows_platform::ExecutionFileSecurity::sddl_equivalent(&current, original),
            "{path:?}: every grant must be restored, now {current}"
        );
    }
}

fn protect_private_dir(path: &Path, owner_sid: &str) {
    let object = aura_windows_platform::ExecutionFileSecurity::open(path, true).unwrap();
    let before = object.capture().unwrap();
    let protected = format!("D:PAI(A;OICI;FA;;;{owner_sid})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)");
    object.apply_exact(&before, &protected).unwrap();
    aura_windows_platform::verify_execution_installation_acl(path, owner_sid, true).unwrap();
}

struct Rig {
    _temp: tempfile::TempDir,
    install_root: PathBuf,
    workspace: PathBuf,
    other_workspace: PathBuf,
    facts: LaunchFacts,
}

impl Rig {
    /// A synthetic installation layout around the real runtime, mirroring the
    /// supervisor chain rig: real locks, a protected journal root and real
    /// NTFS workspaces.
    fn new(files: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        for n in 0..files {
            std::fs::write(workspace.join(format!("{n:04}.txt")), b"content").unwrap();
        }
        let other_workspace = temp.path().join("other-workspace");
        std::fs::create_dir(&other_workspace).unwrap();
        for n in 0..4 {
            std::fs::write(other_workspace.join(format!("other-{n:02}.txt")), b"other").unwrap();
        }
        let install_root = temp.path().join("install");
        let leases = install_root.join("private/execution-leases");
        std::fs::create_dir_all(&leases).unwrap();
        std::fs::create_dir(install_root.join("execution-scratch")).unwrap();
        std::fs::write(install_root.join("private/execution-setup.lock"), []).unwrap();
        std::fs::write(install_root.join("private/execution.lock"), []).unwrap();
        let (owner_sid, _) = aura_windows_platform::current_user_token_identity().unwrap();
        protect_private_dir(&install_root.join("private"), &owner_sid);
        protect_private_dir(&leases, &owner_sid);
        let (runner_image, _) =
            crate::file_binding::capture(&std::env::current_exe().unwrap(), true).unwrap();
        let facts = LaunchFacts {
            root: install_root.clone(),
            installation_id: format!(
                "b3f00000-0000-4000-8000-{:012x}",
                std::process::id() as u64 ^ 0x5a5a_0000_0000u64
            ),
            owner_sid: owner_sid.clone(),
            account: ExecutionAccount {
                name: "aura-runtime-test".into(),
                sid: owner_sid,
                credential_sha256: aura_domain_common::ContentHash::sha256(b"synthetic-credential"),
            },
            runner_image,
        };
        Self { _temp: temp, install_root, workspace, other_workspace, facts }
    }

    fn binding(path: &Path) -> ExecutionFileBinding {
        crate::file_binding::capture(path, false).unwrap().0
    }

    fn journal(&self, nonce: &str) -> PathBuf {
        self.install_root.join("private/execution-leases").join(format!("{nonce}.jsonl"))
    }

    fn scratch_path(&self, nonce: &str) -> PathBuf {
        self.install_root.join("execution-scratch").join(nonce)
    }
}

/// The same observation the independent probe used: cancel once `count`
/// intents are durable.
fn cancel_after_intents(
    journal: PathBuf,
    count: usize,
    flag: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(30);
        while Instant::now() < until {
            if let Ok(text) = std::fs::read_to_string(&journal)
                && text.matches("\"entry\":\"intent\"").count() >= count
            {
                flag.store(true, Ordering::Release);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the walk never journaled {count} intents");
    })
}

fn cancel_signal(flag: &Arc<AtomicBool>) -> GrantSignal {
    let flag = flag.clone();
    GrantSignal::new(move || flag.load(Ordering::Acquire))
}

/// The production prepare sequence: scratch first, then every read root.
fn prepare_scopes<'a>(
    install_root: &'a Path,
    scratch: PathBuf,
    scopes: &'a [ExecutionFileBinding],
    account: &'a str,
    control: &'a GrantControl,
) -> impl FnOnce(&mut FileLease, &mut Option<ExecutionFileBinding>) -> anyhow::Result<()> + 'a {
    move |files, registered_scratch| {
        crate::file_binding::capture(&install_root.join("execution-scratch"), false)?;
        std::fs::create_dir(&scratch)?;
        let (binding, _) = crate::file_binding::capture(&scratch, false)?;
        *registered_scratch = Some(binding.clone());
        files.grant_tree(&binding, account, Some(WRITE), control).finish()?;
        for scope in scopes {
            files.grant_tree(scope, account, Some(WRITE), control).finish()?;
        }
        Ok(())
    }
}

/// The probe's chain: cancel during the workspace enumeration, then keep using
/// the same runtime instance. The halted scope must let the unit settle — the
/// scratch is retired, the journal is released and neither the original nor a
/// different read scope reports a background conflict.
#[test]
fn cancel_during_workspace_enumeration_settles_and_frees_the_runtime() {
    let rig = Rig::new(64);
    let before = snapshot(&rig.workspace);
    let resources = RuntimeResources::default();
    let workspace = Rig::binding(&rig.workspace);
    let first = nonce(0x01);
    let cancelled = Arc::new(AtomicBool::new(false));
    let control = GrantControl::with_budget(Duration::from_secs(60), cancel_signal(&cancelled));
    let watcher = cancel_after_intents(rig.journal(&first), 4, cancelled.clone());
    let allocated = resources.allocate(
        &rig.facts,
        &first,
        &rig.facts.account.sid,
        std::slice::from_ref(&workspace),
        prepare_scopes(
            &rig.install_root,
            rig.scratch_path(&first),
            std::slice::from_ref(&workspace),
            &rig.facts.account.sid,
            &control,
        ),
    );
    watcher.join().unwrap();
    assert!(allocated.is_err(), "a stopped preparation never reports success");
    let counts = resources.resource_counts();
    assert_eq!((counts.active, counts.background), (1, 1), "the partial unit remains owned");

    // The halted scope proves no Execute followed: the same runtime instance
    // settles the unit, restores every captured ACL and retires the scratch.
    assert!(resources.stopped(&first).unwrap(), "the runtime drains");
    assert!(!rig.scratch_path(&first).exists(), "the attempt scratch is retired");
    assert_restored(&before);
    let journal = std::fs::read_to_string(rig.journal(&first)).unwrap();
    assert!(journal.contains("\"entry\":\"scope_halted\""));
    assert!(journal.contains("\"entry\":\"released\""));
    assert_eq!(resources.resource_counts().active, 0);

    // The freed runtime accepts a different read scope for the same account —
    // the conflict guard must not see the settled unit.
    let other = Rig::binding(&rig.other_workspace);
    let second = nonce(0x02);
    let full = GrantControl::unbounded();
    let job = resources
        .allocate(
            &rig.facts,
            &second,
            &rig.facts.account.sid,
            std::slice::from_ref(&other),
            prepare_scopes(
                &rig.install_root,
                rig.scratch_path(&second),
                std::slice::from_ref(&other),
                &rig.facts.account.sid,
                &full,
            ),
        )
        .unwrap();
    assert!(resources.stopped(&second).unwrap());
    drop(job);

    // And it accepts the original scope again.
    let third = nonce(0x03);
    let job = resources
        .allocate(
            &rig.facts,
            &third,
            &rig.facts.account.sid,
            std::slice::from_ref(&workspace),
            prepare_scopes(
                &rig.install_root,
                rig.scratch_path(&third),
                std::slice::from_ref(&workspace),
                &rig.facts.account.sid,
                &full,
            ),
        )
        .unwrap();
    assert!(resources.stopped(&third).unwrap());
    drop(job);
}

/// The halt fact also holds when the first scope — the attempt scratch itself
/// — is the one cancelled mid-enumeration: the workspace walk never starts,
/// and the runtime still settles and frees itself.
#[test]
fn cancel_during_scratch_enumeration_settles_and_frees_the_runtime() {
    let rig = Rig::new(4);
    let resources = RuntimeResources::default();
    let workspace = Rig::binding(&rig.workspace);
    let first = nonce(0x11);
    let cancelled = Arc::new(AtomicBool::new(false));
    let control = GrantControl::with_budget(Duration::from_secs(60), cancel_signal(&cancelled));
    let watcher = cancel_after_intents(rig.journal(&first), 2, cancelled.clone());
    let install_root = rig.install_root.clone();
    let scratch = rig.scratch_path(&first);
    let account = rig.facts.account.sid.clone();
    let workspace_for_grant = workspace.clone();
    let allocated = resources.allocate(
        &rig.facts,
        &first,
        &rig.facts.account.sid,
        std::slice::from_ref(&workspace),
        move |files, registered_scratch| {
            crate::file_binding::capture(&install_root.join("execution-scratch"), false)?;
            std::fs::create_dir(&scratch)?;
            // Fill the attempt scratch so its walk has enough objects to stop
            // between; the halt fact must hold for a partial first scope too.
            for n in 0..32 {
                std::fs::write(scratch.join(format!("{n:02}.txt")), b"x")?;
            }
            let (binding, _) = crate::file_binding::capture(&scratch, false)?;
            *registered_scratch = Some(binding.clone());
            files.grant_tree(&binding, &account, Some(WRITE), &control).finish()?;
            files.grant_tree(&workspace_for_grant, &account, Some(WRITE), &control).finish()?;
            Ok(())
        },
    );
    watcher.join().unwrap();
    assert!(allocated.is_err(), "a stopped preparation never reports success");

    assert!(resources.stopped(&first).unwrap(), "the runtime drains");
    assert!(!rig.scratch_path(&first).exists(), "the attempt scratch is retired");
    let journal = std::fs::read_to_string(rig.journal(&first)).unwrap();
    assert!(journal.contains("\"entry\":\"scope_halted\""));
    assert!(journal.contains("\"entry\":\"released\""));

    let second = nonce(0x12);
    let full = GrantControl::unbounded();
    let other = Rig::binding(&rig.other_workspace);
    let job = resources
        .allocate(
            &rig.facts,
            &second,
            &rig.facts.account.sid,
            std::slice::from_ref(&other),
            prepare_scopes(
                &rig.install_root,
                rig.scratch_path(&second),
                std::slice::from_ref(&other),
                &rig.facts.account.sid,
                &full,
            ),
        )
        .unwrap();
    assert!(resources.stopped(&second).unwrap());
    drop(job);
}

/// An external DACL change during the settle keeps the runtime conservatively
/// retained: the unit is not reported drained, a different read scope still
/// conflicts, and only a repaired tree lets the settle finish.
#[test]
fn external_fault_during_settle_retains_the_runtime_until_repaired() {
    let rig = Rig::new(64);
    let before = snapshot(&rig.workspace);
    let resources = RuntimeResources::default();
    let workspace = Rig::binding(&rig.workspace);
    let first = nonce(0x21);
    let cancelled = Arc::new(AtomicBool::new(false));
    let control = GrantControl::with_budget(Duration::from_secs(60), cancel_signal(&cancelled));
    // Cancel once the apply phase rewrote the workspace root: the grant form
    // on the root is the signal that the batch started.
    let grant_form = format!("(A;OICI;FA;;;{})", rig.facts.account.sid);
    let watched_root = rig.workspace.clone();
    let stop = cancelled.clone();
    let watcher = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(30);
        while Instant::now() < until {
            if acl(&watched_root).contains(&grant_form) {
                stop.store(true, Ordering::Release);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("the apply phase never rewrote the workspace root");
    });
    let allocated = resources.allocate(
        &rig.facts,
        &first,
        &rig.facts.account.sid,
        std::slice::from_ref(&workspace),
        prepare_scopes(
            &rig.install_root,
            rig.scratch_path(&first),
            std::slice::from_ref(&workspace),
            &rig.facts.account.sid,
            &control,
        ),
    );
    watcher.join().unwrap();
    assert!(allocated.is_err(), "a stopped preparation never reports success");

    // External interference: a foreign grant lands on the workspace root
    // before the settle can restore it.
    let object = aura_windows_platform::ExecutionFileSecurity::open(&rig.workspace, false).unwrap();
    let flipped_before = object.capture().unwrap();
    let flipped = object.plan(&[("S-1-5-21-333333-444444-555555-777777", 0x1200a9)]).unwrap();
    object.apply_exact(&flipped_before, &flipped).unwrap();
    resources.stopped(&first).expect_err("the settle refuses a tree it cannot restore");
    let journal = std::fs::read_to_string(rig.journal(&first)).unwrap();
    assert!(!journal.contains("\"entry\":\"released\""), "no cleanup receipt is published");

    // The retained unit keeps its read scope: another workspace conflicts.
    let other = Rig::binding(&rig.other_workspace);
    let conflict = resources
        .allocate(
            &rig.facts,
            &nonce(0x22),
            &rig.facts.account.sid,
            std::slice::from_ref(&other),
            |_, _| Ok(()),
        )
        .err()
        .expect("a retained unit keeps its read scope");
    assert!(
        format!("{conflict:#}").contains("runner.background_read_scope_conflict"),
        "{conflict:#}"
    );

    // Repair the interference; the settle then completes and frees the runtime.
    object.apply_exact(&flipped, &flipped_before).unwrap();
    assert!(resources.stopped(&first).unwrap(), "the repaired runtime drains");
    assert_restored(&before);
    let journal = std::fs::read_to_string(rig.journal(&first)).unwrap();
    assert!(journal.contains("\"entry\":\"released\""));
    let third = nonce(0x23);
    let full = GrantControl::unbounded();
    let job = resources
        .allocate(
            &rig.facts,
            &third,
            &rig.facts.account.sid,
            std::slice::from_ref(&other),
            prepare_scopes(
                &rig.install_root,
                rig.scratch_path(&third),
                std::slice::from_ref(&other),
                &rig.facts.account.sid,
                &full,
            ),
        )
        .unwrap();
    assert!(resources.stopped(&third).unwrap());
    drop(job);
}

/// A journal from before the stop facts existed — intents, but no process
/// boundary, no enumeration fact and no halt — keeps the runtime closed:
/// recovery refuses to guess, and nothing is released or rewritten.
#[test]
fn legacy_journal_without_stop_facts_keeps_the_runtime_closed() {
    let rig = Rig::new(4);
    let resources = RuntimeResources::default();
    let file = rig.workspace.join("0000.txt");
    let original = acl(&file);
    let info = aura_windows_platform::inspect_existing_path(&file, false).unwrap();
    let (root, _) = crate::file_binding::capture(&rig.workspace, false).unwrap();
    let journal = rig.journal("legacy");
    let scope = serde_json::json!({
        "entry": "scope",
        "root": root,
        "account": rig.facts.account.sid,
        "write_sid": WRITE,
    });
    let intent = serde_json::json!({
        "entry": "intent",
        "path": file,
        "volume": info.identity.volume_serial_number,
        "id": info.identity.file_id,
        "before": original,
        "after": original,
    });
    std::fs::write(&journal, format!("{scope}\n{intent}\n")).unwrap();
    let workspace = Rig::binding(&rig.workspace);
    let error = resources
        .allocate(
            &rig.facts,
            &nonce(0x31),
            &rig.facts.account.sid,
            std::slice::from_ref(&workspace),
            |_, _| Ok(()),
        )
        .err()
        .expect("a legacy journal blocks the runtime");
    assert!(format!("{error:#}").contains("runner.legacy_lease_stop_unproven"), "{error:#}");
    assert!(
        !std::fs::read_to_string(&journal).unwrap().contains("\"entry\":\"released\""),
        "the legacy journal stays for reconciliation"
    );
    assert_eq!(acl(&file), original, "nothing was rewritten");
}
