//! 设置 → 系统 → 存储 (`storage.*`): where the data and the installation are and how big, the move
//! waiting for the next start and the last one that did not happen; checking a folder and asking
//! for a move; the old data older versions and other installations left. The move itself happens
//! at the next start, by the launcher or, in a development run, by `aura-core --move-data`
//! (`aura_data_move`); old data is found and removed by `aura_old_data`.
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use aura_data_location::{Locations, Roots, current_record, installation};
use aura_data_move::{
    Choice, Context, StorageCheck, StorageChoice, StorageMoveFailure, StorageMoveRequest,
    StorageOverview, StoragePlace,
};
use aura_domain_common::{CommandInvocationId, display_path};
use aura_old_data::{Installed, OldDataListing, OldDataRemoval, OldDataRemoveRequest, Scope};
use aura_runtime_contract::{BundleNamespace, PRODUCTION_BUNDLE_NAMESPACE};
use aura_workspace_domain::WorkspaceSessionProjection;
use serde_json::Value;

type Workspaces<'a> = &'a dyn Fn() -> Result<WorkspaceSessionProjection>;

/// What a request is checked against: where everything is now, and the installation.
struct Now {
    roots: Roots,
    namespace: BundleNamespace,
    current: Locations,
    /// The installation's folder and whether a new version is on trial.
    installation: Option<(PathBuf, bool)>,
}

impl Now {
    fn read(manifest: &Path) -> Result<Self> {
        let (roots, namespace, current) = crate::locations::confirmed()
            .context("storage.unavailable: 还不知道数据在哪里，请稍后再试。")?;
        let installation = installation(manifest)?.map(|(root, state)| {
            let on_trial = state.pending.as_ref().is_some_and(|pending| *pending != state.active);
            (root, on_trial)
        });
        Ok(Self { roots, namespace, current, installation })
    }

    fn production(&self) -> bool {
        self.namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE
    }

    fn old_data<'a>(&'a self, installed: &'a Installed) -> Scope<'a> {
        Scope { roots: &self.roots, current: &self.namespace, installed }
    }
}

/// What the installations Settings > Apps lists use, read for each request: one may have been
/// installed or removed since.
fn installed() -> Installed {
    Installed::from_entries(
        aura_windows_platform::installer::aura_uninstall_entries()
            .map(|entries| entries.into_iter().map(|(_, folder)| folder).collect()),
    )
}

pub fn dispatch(
    manifest: &Path,
    workspaces: Workspaces<'_>,
    method: &str,
    params: Value,
    command: Option<CommandInvocationId>,
) -> Result<Value> {
    let now = Now::read(manifest)?;
    let changes = matches!(
        method,
        "storage.move.request"
            | "storage.move.withdraw"
            | "storage.failure.dismiss"
            | "storage.old_data.remove"
    );
    ensure!(!changes || command.is_some(), "ipc.command_invocation_id_required");
    match method {
        "storage.overview" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
        }
        "storage.old_data.list" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            let installed = installed();
            let found = aura_old_data::find(&now.old_data(&installed));
            return Ok(serde_json::to_value(OldDataListing::of(&found))?);
        }
        "storage.old_data.remove" => {
            let request: OldDataRemoveRequest = serde_json::from_value(params)?;
            let installed = installed();
            let scope = now.old_data(&installed);
            let removed = aura_old_data::remove(&scope, &request.ids);
            let left = aura_old_data::find(&scope);
            return Ok(serde_json::to_value(OldDataRemoval::of(removed, &left))?);
        }
        "storage.check" => {
            let choice: StorageChoice = serde_json::from_value(params)?;
            return Ok(serde_json::to_value(check(&now, workspaces, &choice)?)?);
        }
        "storage.move.request" => {
            let choice: StorageChoice = serde_json::from_value(params)?;
            let checked = check(&now, workspaces, &choice)?;
            if !checked.refusals.is_empty() {
                bail!("storage.refused: {}", checked.refusals.join("；"));
            }
            let destination = checked.destination.map(PathBuf::from);
            aura_data_move::request(&now.namespace, &now.current, destination)
                .context("storage.not_saved: 搬家请求没有写下")?;
        }
        "storage.move.withdraw" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            aura_data_move::withdraw(&now.namespace, &now.current)
                .context("storage.not_saved: 搬家请求没有取消")?;
        }
        "storage.failure.dismiss" => {
            let _: aura_agent_domain::EmptyToolInput = serde_json::from_value(params)?;
            aura_data_move::dismiss_failure(&now.current.data_root)
                .context("storage.not_saved: 没有清掉")?;
        }
        _ => bail!("storage.unknown_method"),
    }
    Ok(serde_json::to_value(overview(&now)?)?)
}

fn overview(now: &Now) -> Result<StorageOverview> {
    let current = &now.current;
    let data_path = current.location.as_ref().unwrap_or(&current.data_root);
    let requested = aura_data_move::requested(&now.namespace, current)?
        .map(|destination| StorageMoveRequest { destination: destination.as_deref().map(shown) });
    let failure =
        aura_data_move::read_failure(&current.data_root).map(|failure| StorageMoveFailure {
            at: failure.at,
            destination: failure.destination.as_deref().map(shown),
            reason: failure.reason,
        });
    // The record, not the start's copy of it: the old copies may have gone since.
    let old_copies = current_record(&current.record, &now.namespace)
        .ok()
        .flatten()
        .is_some_and(|record| !record.pending_removal.is_empty());
    Ok(StorageOverview {
        data: StoragePlace {
            path: shown(data_path),
            bytes: aura_data_move::data_size(current, now.production()).unwrap_or(0),
        },
        data_moved: current.location.is_some(),
        install: now.installation.as_ref().map(|(root, _)| StoragePlace {
            path: shown(root),
            bytes: aura_data_move::folder_size(root),
        }),
        requested,
        failure,
        old_copies,
    })
}

fn check(now: &Now, workspaces: Workspaces<'_>, choice: &StorageChoice) -> Result<StorageCheck> {
    let workspaces: Vec<PathBuf> = workspaces()?
        .workspaces
        .iter()
        .map(|workspace| PathBuf::from(shown(Path::new(&workspace.canonical_root))))
        .collect();
    let context = Context {
        roots: &now.roots,
        namespace: &now.namespace,
        current: &now.current,
        install_root: now.installation.as_ref().map(|(root, _)| root.as_path()),
        workspaces: &workspaces,
        on_trial: now.installation.as_ref().is_some_and(|(_, on_trial)| *on_trial),
        production: now.production(),
    };
    let choice = choice
        .folder
        .as_ref()
        .map_or(Choice::DefaultPlace, |folder| Choice::Folder(PathBuf::from(folder)));
    Ok(match aura_data_move::check(&context, &choice) {
        Ok(accepted) => StorageCheck {
            destination: accepted.destination.as_deref().map(shown),
            bytes: accepted.plan.bytes,
            parts: aura_data_move::parts_of(&accepted.plan),
            needed: accepted.needed,
            free: accepted.free,
            refusals: Vec::new(),
        },
        Err(refusals) => StorageCheck {
            destination: match &choice {
                Choice::Folder(folder) => Some(shown(folder)),
                Choice::DefaultPlace => None,
            },
            bytes: 0,
            parts: Vec::new(),
            needed: 0,
            free: 0,
            refusals: refusals.iter().map(ToString::to_string).collect(),
        },
    })
}

fn shown(path: &Path) -> String {
    display_path(path)
}
