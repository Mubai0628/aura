#![forbid(unsafe_code)]

mod data_backup;
mod data_slimming;
mod install_state;
mod trial;
mod version;

use std::{
    collections::BTreeMap,
    env,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use aura_domain_common::{RuntimeInstanceId, UtcTimestamp};
use aura_runtime_contract::{ActiveInstallState, BuildComponentManifest, ReleaseDescriptor};
use aura_windows_platform::{
    RecoveryAction, RecoveryChoice, RootJob, choose_diagnostic_export_path, show_recovery_dialog,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use data_backup::{RESTORE_FILE, RestoreRequest, backup_folder};
pub use install_state::newer;
pub use install_state::{
    Start, abandon, choose, kept, promote, read as read_install_state, remove_unkept_versions,
    replace as replace_file, roll_back, write as write_install_state,
};
pub use trial::{FailedTrial, TrialFailure};
pub use version::{VersionSelection, resolve_version};

pub const APP_CRASH_ESCALATED_EXIT_CODE: u32 = 78;
/// The desktop asks to be started again, as after an update was applied.
pub const APP_RESTART_EXIT_CODE: u32 = 75;
const CRASH_WINDOW: Duration = Duration::from_secs(10 * 60);
const MAX_DIAGNOSTIC_LOG_BYTES: u64 = 1024 * 1024;
const TRIAL_POLL: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
pub struct InstallSelection {
    pub root: PathBuf,
    pub version_directory: PathBuf,
    pub desktop_executable: PathBuf,
    pub active: ActiveInstallState,
    pub manifest: BuildComponentManifest,
    pub manifest_json: Value,
    pub descriptor: ReleaseDescriptor,
}

impl InstallSelection {
    fn new(root: PathBuf, active: ActiveInstallState, selected: VersionSelection) -> Self {
        Self {
            root,
            version_directory: selected.directory,
            desktop_executable: selected.desktop_executable,
            active,
            manifest: selected.manifest,
            manifest_json: selected.manifest_json,
            descriptor: selected.descriptor,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CrashState {
    schema_version: u32,
    components: BTreeMap<String, Vec<u64>>,
}

#[derive(Clone, Debug, Serialize)]
struct ExitRecord {
    component: String,
    exit_code: u32,
    occurred_at: String,
}

/// The active version of the installation `AuraLauncher.exe` sits in, checked against
/// `active.json`.
pub fn resolve_install(launcher_path: &Path) -> Result<InstallSelection> {
    let root =
        launcher_path.parent().context("AuraLauncher.exe has no parent directory")?.to_path_buf();
    let active = install_state::read(&root)?;
    let selected = resolve_version(&root, &active.active, Some(&active.release_descriptor))?;
    Ok(InstallSelection::new(root, active, selected))
}

/// What one start of the desktop runs: the active version, or the waiting one on trial.
struct Launch {
    selection: InstallSelection,
    trial: bool,
}

/// Picks what to start. A waiting version that does not check out is recorded as a failed trial
/// and dropped, and the active version starts instead.
fn prepare(root: &Path, state: &ActiveInstallState, log: &Path) -> Result<Launch> {
    if let Start::Trial(version) = choose(state) {
        let checked = resolve_version(root, &version, None).and_then(|selected| {
            if selected.manifest.bundle_namespace == state.release_descriptor.bundle_namespace {
                Ok(selected)
            } else {
                anyhow::bail!("the waiting version belongs to another Aura installation")
            }
        });
        match checked {
            Ok(selected) => {
                append_log(
                    log,
                    json!({
                        "timestamp": UtcTimestamp::now(),
                        "component": "launcher",
                        "event": "trial_starting",
                        "version": version,
                        "kept_version": state.active,
                    }),
                )?;
                return Ok(Launch {
                    selection: InstallSelection::new(root.to_path_buf(), state.clone(), selected),
                    trial: true,
                });
            }
            Err(error) => {
                fail_trial(root, state, &version, TrialFailure::Invalid, None, &error, log)?;
            }
        }
    }
    let state = install_state::read(root)?;
    let selected = resolve_version(root, &state.active, Some(&state.release_descriptor))?;
    Ok(Launch {
        selection: InstallSelection::new(root.to_path_buf(), state, selected),
        trial: false,
    })
}

fn fail_trial(
    root: &Path,
    state: &ActiveInstallState,
    version: &str,
    failure: TrialFailure,
    exit_code: Option<u32>,
    error: &dyn std::fmt::Display,
    log: &Path,
) -> Result<()> {
    install_state::write(root, &abandon(state))?;
    trial::record_failed(
        &local_data_root(&state.release_descriptor.bundle_namespace)?,
        &FailedTrial {
            version: version.to_owned(),
            kept_version: state.active.clone(),
            failure,
            exit_code,
            detail: error.to_string(),
            at: UtcTimestamp::now(),
        },
    )?;
    append_log(
        log,
        json!({
            "timestamp": UtcTimestamp::now(),
            "component": "launcher",
            "event": "trial_abandoned",
            "version": version,
            "kept_version": state.active,
            "failure": failure,
            "exit_code": exit_code,
            "detail": error.to_string(),
        }),
    )
}

/// Before a trial: going back to an older version puts the copy from before the current version
/// back when the user asked for that.
fn restore_data(
    data_root: &Path,
    state: &ActiveInstallState,
    version: &str,
    log: &Path,
) -> Result<()> {
    let request = data_backup::take_request(data_root);
    if !newer(version, &state.active)
        && let Some(request) = request
        && request.before_version == state.active
        && data_backup::restore(data_root, &request.before_version)?
    {
        append_log(
            log,
            json!({
                "timestamp": UtcTimestamp::now(),
                "component": "launcher",
                "event": "data_restored",
                "before_version": request.before_version,
            }),
        )?;
    }
    Ok(())
}

/// Before a newer version's first start, after its slimming: a copy of the stores it will migrate
/// or leave unreadable to the active version (its own Core in `version_directory` says which).
/// `false` when the user chose not to start the newer version without a copy.
fn back_up_data(
    data_root: &Path,
    state: &ActiveInstallState,
    version: &str,
    version_directory: &Path,
    log: &Path,
) -> Result<bool> {
    if newer(version, &state.active) {
        let started = std::time::Instant::now();
        let pending = data_backup::pending_stores(
            &version_directory.join("aura-core.exe"),
            data_root,
            &state.active,
        );
        let taken = data_backup::take(data_root, version, pending.as_ref());
        let event = match &taken {
            Ok(data_backup::Taken::Copied { stores, bytes }) => json!({
                "event": "data_backed_up",
                "version": version,
                "stores": stores,
                "bytes": bytes,
                "elapsed_ms": started.elapsed().as_millis() as u64,
                "migration_known": pending.is_some(),
            }),
            Ok(data_backup::Taken::NothingToCopy) => json!({
                "event": "data_backup_skipped",
                "version": version,
                "reason": "no_store_migration",
            }),
            Ok(data_backup::Taken::NoRoom { needed, free }) => json!({
                "event": "data_backup_skipped",
                "version": version,
                "reason": "insufficient_space",
                "needed_bytes": needed,
                "free_bytes": free,
            }),
            Err(error) => {
                json!({ "event": "data_backup_failed", "version": version, "detail": format!("{error:#}") })
            }
        };
        append_log(
            log,
            json!({ "timestamp": UtcTimestamp::now(), "component": "launcher", "data": event }),
        )?;
        const MIB: u64 = 1024 * 1024;
        let problem = match taken {
            Ok(data_backup::Taken::NoRoom { needed, free }) => (
                "磁盘空间不够备份数据",
                format!("需要约 {} MB，这个磁盘只剩 {} MB。", needed.div_ceil(MIB), free / MIB),
            ),
            Err(error) => ("更新前备份数据没有成功", format!("{}。", error.root_cause())),
            Ok(_) => return Ok(true),
        };
        return start_without_backup(version, problem);
    }
    Ok(true)
}

/// Clears what older versions wrote into the execution store and nothing reads, before the desktop
/// opens it (`data_slimming`). The outcome is logged; a failed run never keeps Aura from starting.
fn slim_data(data_root: &Path, version_directory: &Path, log: &Path) -> Result<()> {
    let started = std::time::Instant::now();
    let outcome =
        aura_windows_platform::with_progress_window("Aura", "正在整理数据", |progress| {
            data_slimming::run(&version_directory.join("aura-core.exe"), data_root, progress)
        });
    let elapsed_ms = started.elapsed().as_millis() as u64;
    let event = match outcome {
        data_slimming::Outcome::Report(report) => {
            json!({ "event": "data_slimmed", "report": report, "elapsed_ms": elapsed_ms })
        }
        data_slimming::Outcome::Failed(detail) => {
            json!({ "event": "data_slim_failed", "detail": detail, "elapsed_ms": elapsed_ms })
        }
    };
    append_log(
        log,
        json!({ "timestamp": UtcTimestamp::now(), "component": "launcher", "data": event }),
    )
}

/// No copy could be taken before `version` migrates the data: the user decides whether it starts
/// anyway. Closing the window starts nothing, and the next start tries the copy again.
fn start_without_backup(version: &str, (instruction, cause): (&str, String)) -> Result<bool> {
    let details = format!(
        "新版本 {version} 第一次启动会升级数据，启动前要先备份。{cause}\n可以先退出，处理好后再启动 Aura；也可以不备份直接启动。"
    );
    let action = show_recovery_dialog(&RecoveryChoice {
        instruction,
        details: &details,
        actions: &[RecoveryAction::Exit, RecoveryAction::StartWithoutBackup],
        previous_version: None,
    })
    .context("failed to show the backup window")?;
    Ok(action == RecoveryAction::StartWithoutBackup)
}

/// The previous version, when it is there and checks out.
fn intact_previous(root: &Path, state: &ActiveInstallState) -> Option<String> {
    let previous = roll_back(state)?.pending?;
    resolve_version(root, &previous, None).ok().map(|_| previous)
}

/// The active version does not check out: the window says why and offers the previous version.
fn startup_failure(root: &Path, state: &ActiveInstallState, error: &anyhow::Error) -> Result<bool> {
    let previous = intact_previous(root, state);
    let details = format!(
        "当前版本 {} 的程序文件不完整或被改动：{error:#}\n可以回到上一版本，或重新运行 Aura 安装程序修复。",
        state.active
    );
    let action = show_recovery_dialog(&RecoveryChoice {
        instruction: "Aura 无法启动",
        details: &details,
        actions: &[RecoveryAction::RollBack, RecoveryAction::Exit],
        previous_version: previous.as_deref(),
    })
    .context("failed to show the recovery window")?;
    if action == RecoveryAction::RollBack
        && let Some(back) = roll_back(state)
    {
        install_state::write(root, &back)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn run_launcher() -> Result<()> {
    let launcher_path = env::current_exe().context("failed to resolve AuraLauncher.exe")?;
    let root =
        launcher_path.parent().context("AuraLauncher.exe has no parent directory")?.to_path_buf();
    let mut exit_records = Vec::new();
    let launcher_runtime_id = RuntimeInstanceId::new();
    let mut safe_mode = env::args().any(|argument| argument == "--safe-mode");

    loop {
        let state = install_state::read(&root)?;
        let log_directory =
            local_data_root(&state.release_descriptor.bundle_namespace)?.join("logs");
        fs::create_dir_all(&log_directory).context("failed to create Aura log directory")?;
        let launcher_log = log_directory.join("launcher.jsonl");
        let launch = match prepare(&root, &state, &launcher_log) {
            Ok(launch) => launch,
            Err(error) => {
                append_log(
                    &launcher_log,
                    json!({
                        "timestamp": UtcTimestamp::now(),
                        "component": "launcher",
                        "event": "version_rejected",
                        "version": state.active,
                        "detail": format!("{error:#}"),
                    }),
                )?;
                if startup_failure(&root, &install_state::read(&root)?, &error)? {
                    continue;
                }
                return Ok(());
            }
        };
        let selection = launch.selection;
        let data_root = local_data_root(&selection.manifest.bundle_namespace)?;
        let version = &selection.manifest.product_version;
        if launch.trial {
            restore_data(&data_root, &state, version, &launcher_log)?;
        }
        // Slimmed before the copy, so the copy kept until the next version is the slimmed store.
        // The slimming changes no structure; the version's Core migrates the store after the copy.
        if data_slimming::owed(&data_root, version, launch.trial) {
            slim_data(&data_root, &selection.version_directory, &launcher_log)?;
        }
        if launch.trial
            && !back_up_data(
                &data_root,
                &state,
                version,
                &selection.version_directory,
                &launcher_log,
            )?
        {
            return Ok(());
        }
        let crash_state_path = data_root.join("launcher").join("crash-state.v1.json");
        let mut crash_state = load_crash_state(&crash_state_path);
        append_log(
            &launcher_log,
            json!({
                "timestamp": UtcTimestamp::now(),
                "component": "launcher",
                "event": "desktop_starting",
                "bundle_namespace": selection.manifest.bundle_namespace,
                "release_bundle_id": selection.manifest.release_bundle_id,
                "product_version": selection.manifest.product_version,
                "trial": launch.trial,
                "safe_mode": safe_mode,
            }),
        )?;
        // Taken before the start, with a second's allowance for file time rounding: a report the
        // new desktop writes at once still counts, one from an earlier run does not.
        let started = SystemTime::now() - Duration::from_secs(1);
        let root_job = RootJob::create().context("failed to create Root Job")?;
        let mut arguments = vec![format!("--aura-parent-runtime-id={launcher_runtime_id}")];
        if safe_mode {
            arguments.push("--safe-mode".to_owned());
        }
        let process = root_job
            .spawn_assigned(&selection.desktop_executable, &arguments, &selection.version_directory)
            .context("failed to start Aura.exe inside the Root Job")?;
        append_log(
            &launcher_log,
            json!({
                "timestamp": UtcTimestamp::now(),
                "component": "launcher",
                "event": "desktop_started",
                "process_id": process.process_id(),
            }),
        )?;
        let mut on_trial = launch.trial;
        let exit_code = loop {
            // The exit is read before the report is checked, so a report written just before
            // the exit still counts.
            let exited = process.try_exit_code().context("failed while waiting for Aura.exe")?;
            if on_trial
                && trial::reported_ready(&data_root, selection.manifest.release_bundle_id, started)
            {
                let current = install_state::read(&root)?;
                let promoted = promote(
                    &current,
                    &selection.manifest.product_version,
                    selection.descriptor.clone(),
                );
                install_state::write(&root, &promoted)?;
                let removed = remove_unkept_versions(&root, &promoted);
                append_log(
                    &launcher_log,
                    json!({
                        "timestamp": UtcTimestamp::now(),
                        "component": "launcher",
                        "event": "trial_promoted",
                        "version": promoted.active,
                        "previous": promoted.previous,
                        "removed_versions": removed,
                    }),
                )?;
                on_trial = false;
            }
            if let Some(code) = exited {
                break code;
            }
            thread::sleep(TRIAL_POLL);
        };
        drop(root_job);
        let component =
            if exit_code == APP_CRASH_ESCALATED_EXIT_CODE { "core" } else { "electron_main" };
        append_log(
            &launcher_log,
            json!({
                "timestamp": UtcTimestamp::now(),
                "component": component,
                "event": "process_exited",
                "exit_code": exit_code,
            }),
        )?;
        if exit_code == APP_RESTART_EXIT_CODE {
            clear_component_crashes(&mut crash_state, component);
            save_crash_state(&crash_state_path, &crash_state)?;
            continue;
        }
        if on_trial {
            if exit_code == 0 {
                // Closed before it was ready: the version stays waiting for the next start.
                return Ok(());
            }
            let current = install_state::read(&root)?;
            let version = &selection.manifest.product_version;
            if newer(version, &current.active) && data_backup::restore(&data_root, version)? {
                append_log(
                    &launcher_log,
                    json!({
                        "timestamp": UtcTimestamp::now(),
                        "component": "launcher",
                        "event": "data_restored",
                        "before_version": version,
                    }),
                )?;
            }
            fail_trial(
                &root,
                &current,
                &selection.manifest.product_version,
                TrialFailure::Crashed,
                Some(exit_code),
                &format!("exit code {exit_code}"),
                &launcher_log,
            )?;
            continue;
        }
        exit_records.push(ExitRecord {
            component: component.to_owned(),
            exit_code,
            occurred_at: UtcTimestamp::now().to_string(),
        });
        if exit_code == 0 {
            clear_component_crashes(&mut crash_state, component);
            save_crash_state(&crash_state_path, &crash_state)?;
            return Ok(());
        }

        let crash_count = record_crash(&mut crash_state, component);
        save_crash_state(&crash_state_path, &crash_state)?;
        if crash_count < 3 && exit_code != APP_CRASH_ESCALATED_EXIT_CODE {
            thread::sleep(if crash_count == 1 {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(2)
            });
            continue;
        }

        let current = install_state::read(&root)?;
        let previous = intact_previous(&root, &current);
        loop {
            let details = format!(
                "组件 {component} 在滚动 10 分钟窗口内重复失败。\n退出代码：{exit_code}\n当前版本：{}",
                current.active
            );
            let action = show_recovery_dialog(&RecoveryChoice {
                instruction: "Aura 已停止以避免重复崩溃",
                details: &details,
                actions: &[
                    RecoveryAction::Restart,
                    RecoveryAction::SafeMode,
                    RecoveryAction::RollBack,
                    RecoveryAction::ExportDiagnostics,
                    RecoveryAction::Exit,
                ],
                previous_version: previous.as_deref(),
            })
            .context("failed to show the recovery window")?;
            match action {
                RecoveryAction::Restart => {
                    clear_component_crashes(&mut crash_state, component);
                    save_crash_state(&crash_state_path, &crash_state)?;
                    safe_mode = false;
                    break;
                }
                RecoveryAction::SafeMode => {
                    clear_component_crashes(&mut crash_state, component);
                    save_crash_state(&crash_state_path, &crash_state)?;
                    safe_mode = true;
                    break;
                }
                RecoveryAction::RollBack => {
                    if let Some(back) = roll_back(&current) {
                        install_state::write(&root, &back)?;
                    }
                    clear_component_crashes(&mut crash_state, component);
                    save_crash_state(&crash_state_path, &crash_state)?;
                    safe_mode = false;
                    break;
                }
                RecoveryAction::ExportDiagnostics => {
                    if let Some(path) = choose_diagnostic_export_path("aura-p01-diagnostics.json")?
                    {
                        export_diagnostics(&path, &selection, &exit_records, &log_directory)?;
                    }
                }
                // Not offered here; anything unexpected leaves the app closed.
                RecoveryAction::Exit | RecoveryAction::StartWithoutBackup => return Ok(()),
            }
        }
    }
}

pub fn export_diagnostics(
    output: &Path,
    selection: &InstallSelection,
    exit_records: &[impl Serialize],
    log_directory: &Path,
) -> Result<()> {
    let mut logs = BTreeMap::new();
    for name in ["launcher.jsonl", "main.jsonl", "core.jsonl"] {
        let path = log_directory.join(name);
        if path.is_file() {
            logs.insert(name, read_tail(&path, MAX_DIAGNOSTIC_LOG_BYTES)?);
        }
    }
    let diagnostic = json!({
        "schema_version": 1,
        "exported_at": UtcTimestamp::now(),
        "active": {
            "active": selection.active.active,
            "release_descriptor": selection.active.release_descriptor,
            "previous": selection.active.previous,
            "pending": selection.active.pending,
            "last_known_good": selection.active.last_known_good,
        },
        "component_manifest": selection.manifest_json,
        "exit_records": exit_records,
        "logs": logs,
    });
    install_state::replace(output, &serde_json::to_vec_pretty(&diagnostic)?)
        .with_context(|| format!("failed to export diagnostics to {}", output.display()))
}

fn local_data_root(namespace: &aura_runtime_contract::BundleNamespace) -> Result<PathBuf> {
    let local = env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not defined")?;
    Ok(PathBuf::from(local).join("Aura").join("namespaces").join(namespace.object_name_hash()))
}

fn load_crash_state(path: &Path) -> CrashState {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CrashState>(&bytes).ok())
        .filter(|state| state.schema_version == 1)
        .unwrap_or(CrashState { schema_version: 1, components: BTreeMap::new() })
}

fn save_crash_state(path: &Path, state: &CrashState) -> Result<()> {
    install_state::replace(path, &serde_json::to_vec_pretty(state)?)
        .with_context(|| format!("failed to persist {}", path.display()))
}

fn record_crash(state: &mut CrashState, component: &str) -> usize {
    let now = unix_seconds();
    let window_start = now.saturating_sub(CRASH_WINDOW.as_secs());
    let records = state.components.entry(component.to_owned()).or_default();
    records.retain(|timestamp| *timestamp >= window_start);
    records.push(now);
    records.len()
}

fn clear_component_crashes(state: &mut CrashState, component: &str) {
    state.components.remove(component);
}

fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn append_log(path: &Path, value: Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, &value)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

fn read_tail(path: &Path, maximum: u64) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let length = file.metadata()?.len();
    if length > maximum {
        use std::io::Seek;
        file.seek(io::SeekFrom::Start(length - maximum))?;
    }
    let mut content = String::new();
    file.take(maximum).read_to_string(&mut content)?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::{CrashState, clear_component_crashes, record_crash, resolve_install};
    use crate::version::validate_version_segment;
    use aura_domain_common::{ContentHash, ContentHashAlgorithm, ContractVersion, ReleaseBundleId};
    use aura_runtime_contract::{
        ActiveInstallState, BuildComponent, BuildComponentKind, BuildComponentManifest,
        BundleNamespace, ReleaseChannel, ReleaseDescriptor, ReleaseIdentity,
    };
    use sha2::{Digest, Sha256};

    #[test]
    fn version_segment_rejects_path_traversal() {
        assert!(validate_version_segment("0.0.0-dev").is_ok());
        assert!(validate_version_segment("../escape").is_err());
        assert!(validate_version_segment(r"C:\escape").is_err());
    }

    #[test]
    fn third_component_crash_escalates_and_can_be_cleared() {
        let mut state = CrashState { schema_version: 1, components: Default::default() };
        assert_eq!(record_crash(&mut state, "core"), 1);
        assert_eq!(record_crash(&mut state, "core"), 2);
        assert_eq!(record_crash(&mut state, "core"), 3);
        clear_component_crashes(&mut state, "core");
        assert_eq!(record_crash(&mut state, "core"), 1);
    }

    #[test]
    fn active_release_identity_hashes_original_manifest_bytes() {
        let install = tempfile::tempdir().expect("temporary install root");
        let version = "1.0.0";
        let version_directory = install.path().join("versions").join(version);
        std::fs::create_dir_all(&version_directory).expect("version directory");
        let desktop_bytes = b"fixture desktop";
        let core_bytes = b"fixture core";
        std::fs::write(version_directory.join("Aura.exe"), desktop_bytes).expect("desktop");
        std::fs::write(version_directory.join("aura-core.exe"), core_bytes).expect("core");
        let release_bundle_id = ReleaseBundleId::new();
        let bundle_namespace: BundleNamespace =
            "aura.desktop.test.launcher".parse().expect("test namespace");
        let component = |component_id: &str,
                         kind: BuildComponentKind,
                         relative_path: &str,
                         bytes: &[u8]| BuildComponent {
            component_id: component_id.to_owned(),
            kind,
            version: version.to_owned(),
            relative_path: relative_path.to_owned(),
            content_hash: ContentHash::new(
                ContentHashAlgorithm::Sha256,
                format!("{:x}", Sha256::digest(bytes)),
            )
            .expect("component hash"),
            protocol_version: Some(ContractVersion::CURRENT),
        };
        let manifest = BuildComponentManifest {
            manifest_version: ContractVersion::CURRENT,
            bundle_namespace: bundle_namespace.clone(),
            release_bundle_id,
            release_channel: ReleaseChannel::Nightly,
            product_version: version.to_owned(),
            build_commit: "fixture".to_owned(),
            target_triple: "x86_64-pc-windows-msvc".to_owned(),
            components: vec![
                component("aura-desktop", BuildComponentKind::Desktop, "Aura.exe", desktop_bytes),
                component("aura-core", BuildComponentKind::Core, "aura-core.exe", core_bytes),
            ],
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).expect("manifest bytes");
        std::fs::write(version_directory.join("component-manifest.json"), &manifest_bytes)
            .expect("manifest");
        let component_manifest_hash = ContentHash::new(
            ContentHashAlgorithm::Sha256,
            format!("{:x}", Sha256::digest(&manifest_bytes)),
        )
        .expect("manifest hash");
        let active_path = install.path().join("active.json");
        std::fs::write(
            &active_path,
            serde_json::to_vec(&ActiveInstallState {
                active: version.to_owned(),
                release_descriptor: ReleaseDescriptor {
                    bundle_namespace,
                    identity: ReleaseIdentity {
                        release_bundle_id,
                        component_manifest_hash: component_manifest_hash.clone(),
                    },
                },
                previous: None,
                pending: None,
                last_known_good: Some(version.to_owned()),
            })
            .expect("active metadata"),
        )
        .expect("active metadata");
        let launcher_path = install.path().join("AuraLauncher.exe");
        resolve_install(&launcher_path).expect("matching active release identity");

        let mut changed_bytes = manifest_bytes;
        changed_bytes.push(b'\n');
        std::fs::write(version_directory.join("component-manifest.json"), changed_bytes)
            .expect("changed manifest");
        assert!(resolve_install(&launcher_path).is_err());

        std::fs::write(
            active_path,
            br#"{"active":"1.0.0","previous":null,"pending":null,"last_known_good":"1.0.0"}"#,
        )
        .expect("legacy active metadata");
        assert!(resolve_install(&launcher_path).is_err());
    }
}
