//! Installing, upgrading and repairing from a release archive. Files are extracted beside the
//! installation first and moved into place only once complete; the launcher then starts an
//! upgrade as a trial, so a version that does not start leaves the installed one running.
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use aura_domain_common::{ContentHash, ContentHashAlgorithm, ReleaseBundleId};
use aura_launcher::{
    read_install_state, remove_unkept_versions, resolve_version, write_install_state,
};
use aura_runtime_contract::{
    ActiveInstallState, BuildComponentKind, BuildComponentManifest, ReleaseDescriptor,
    ReleaseIdentity, ReleaseTrustDomain,
};
use aura_windows_platform::installer::{RunningProgram, Shortcut, UninstallEntry};
use sha2::{Digest, Sha256};

use crate::{
    host::{Host, shortcut_name},
    payload::{self, Archive, ICON, LAUNCHER},
};

pub const UNINSTALLER: &str = "Uninstall Aura.exe";
pub const PUBLISHER: &str = "Aura Project";

/// Aura is running from the installation, so its files cannot be replaced or removed now.
#[derive(Debug)]
pub struct AuraIsRunning(pub Vec<RunningProgram>);

impl fmt::Display for AuraIsRunning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Aura 正在运行，请先关闭 Aura。")
    }
}

impl std::error::Error for AuraIsRunning {}

pub fn ensure_not_running(host: &dyn Host, root: &Path) -> Result<()> {
    let running = host.running_in(root)?;
    if running.is_empty() { Ok(()) } else { Err(AuraIsRunning(running).into()) }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Signature {
    /// Checked against the release root built into this program.
    Verified,
    /// A development or test release, which no release root signs.
    NotRequired,
}

pub struct Release {
    pub version: String,
    pub manifest: BuildComponentManifest,
    pub descriptor: ReleaseDescriptor,
    pub signature: Signature,
}

/// Reads the release an archive carries and checks its manifest signature: a production release
/// must be signed by the release root this program carries.
pub fn read_release(archive: &mut Archive) -> Result<Release> {
    let version = payload::version_of(archive)?;
    let folder = format!("versions/{version}");
    let bytes = payload::read_entry(archive, &format!("{folder}/component-manifest.json"))?
        .context("the release has no component manifest")?;
    let manifest: BuildComponentManifest =
        serde_json::from_slice(&bytes).context("the release's component manifest is invalid")?;
    ensure!(manifest.product_version == version, "the release's manifest names another version");
    let descriptor = ReleaseDescriptor {
        bundle_namespace: manifest.bundle_namespace.clone(),
        identity: ReleaseIdentity {
            release_bundle_id: manifest.release_bundle_id,
            component_manifest_hash: ContentHash::new(
                ContentHashAlgorithm::Sha256,
                format!("{:x}", Sha256::digest(&bytes)),
            )?,
        },
    };
    let signature = match manifest.bundle_namespace.trust_domain() {
        Some(ReleaseTrustDomain::Production) => {
            let signature =
                payload::read_entry(archive, &format!("{folder}/component-manifest.p7s"))?
                    .context("this release is not signed")?;
            aura_windows_platform::verify_release_manifest(&bytes, &signature, &descriptor)
                .context("this release's signature is not valid")?;
            Signature::Verified
        }
        Some(_) => Signature::NotRequired,
        None => bail!("this release belongs to no Aura installation"),
    };
    Ok(Release { version, manifest, descriptor, signature })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Kind {
    Fresh,
    Upgrade { from: String },
    Repair,
    Downgrade { from: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    pub root: PathBuf,
    pub kind: Kind,
}

fn compare(left: &str, right: &str) -> std::cmp::Ordering {
    let parts = |text: &str| -> Vec<u64> {
        text.split('.').map(|part| part.parse().unwrap_or(0)).collect()
    };
    parts(left).cmp(&parts(right))
}

/// Where the release goes and what installing it means there. An existing installation of the
/// same namespace is found from its Settings > Apps entry, or in the default folder.
pub fn plan(host: &dyn Host, release: &Release, chosen: Option<PathBuf>) -> Result<Plan> {
    let namespace = &release.manifest.bundle_namespace;
    let default = host.default_root(namespace)?;
    let existing = host
        .installed_root(namespace)?
        .filter(|root| root.join("active.json").is_file())
        .or_else(|| default.join("active.json").is_file().then(|| default.clone()));
    if let Some(root) = existing {
        let state = read_install_state(&root)?;
        let kind = match compare(&release.version, &state.active) {
            std::cmp::Ordering::Greater => Kind::Upgrade { from: state.active },
            std::cmp::Ordering::Equal => Kind::Repair,
            std::cmp::Ordering::Less => Kind::Downgrade { from: state.active },
        };
        return Ok(Plan { root, kind });
    }
    let root = chosen.unwrap_or(default);
    if root.exists() && fs::read_dir(&root)?.next().is_some() {
        bail!("{} 不是空文件夹，请选择一个空文件夹或新文件夹。", root.display());
    }
    Ok(Plan { root, kind: Kind::Fresh })
}

pub struct Options {
    pub desktop_shortcut: bool,
    /// Applying an update from inside the running installation: the running version's files are
    /// not touched, and the running launcher and uninstaller are moved aside before their
    /// replacements move in.
    pub while_running: bool,
}

/// Leftovers of an earlier run: moved-aside programs that were still running then, and staging
/// folders a stopped run left behind.
fn clear_leftovers(root: &Path) {
    for folder in [root.to_path_buf(), root.join("versions")] {
        for entry in fs::read_dir(&folder).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(".replaced-") || name.starts_with(".staging-") {
                let path = entry.path();
                let _ = fs::remove_dir_all(&path).or_else(|_| fs::remove_file(&path));
            }
        }
    }
}

fn nonce() -> String {
    format!("{}", ReleaseBundleId::new()).to_lowercase()
}

/// Moves `from` over `to`, keeping what was at `to` beside it until the caller says the move held.
struct Replaced {
    kept: Option<PathBuf>,
    to: PathBuf,
}

impl Replaced {
    fn rename(from: &Path, to: &Path, keep_as: PathBuf) -> Result<Self> {
        let kept = if to.exists() {
            fs::rename(to, &keep_as)
                .with_context(|| format!("cannot move {} aside", to.display()))?;
            Some(keep_as)
        } else {
            None
        };
        if let Err(error) = fs::rename(from, to) {
            if let Some(kept) = &kept {
                let _ = fs::rename(kept, to);
            }
            return Err(error).with_context(|| format!("cannot move {} into place", to.display()));
        }
        Ok(Self { kept, to: to.to_path_buf() })
    }

    /// What was moved in goes, and what was there before comes back.
    fn undo(self) {
        let _ = fs::remove_dir_all(&self.to).or_else(|_| fs::remove_file(&self.to));
        if let Some(kept) = self.kept {
            let _ = fs::rename(kept, &self.to);
        }
    }

    fn keep(self) {
        if let Some(kept) = self.kept {
            let _ = fs::remove_dir_all(&kept).or_else(|_| fs::remove_file(&kept));
        }
    }
}

fn folder_size(folder: &Path) -> u64 {
    fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_size(&entry.path()),
            Ok(_) => entry.metadata().map(|metadata| metadata.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Installs the release according to `plan`. `program` is this setup program, whose bytes without
/// the release become the installation's uninstaller when the release names no setup program.
pub fn install(
    host: &dyn Host,
    program: &Path,
    archive: &mut Archive,
    release: &Release,
    plan: &Plan,
    options: &Options,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let root = &plan.root;
    if !options.while_running {
        ensure_not_running(host, root)?;
    }
    clear_leftovers(root);
    fs::create_dir_all(root.join("versions"))?;
    let staging = root.join(format!(".staging-{}", nonce()));
    let result = (|| -> Result<()> {
        payload::extract(archive, &staging, progress)?;
        let version_folder = root.join("versions").join(&release.version);
        let moved = Replaced::rename(
            &staging.join("versions").join(&release.version),
            &version_folder,
            root.join("versions").join(format!(".replaced-{}-{}", release.version, nonce())),
        )?;
        match resolve_version(root, &release.version, Some(&release.descriptor)) {
            Ok(_) => moved.keep(),
            Err(error) => {
                moved.undo();
                return Err(error.context("the installed files do not match the release"));
            }
        }
        // The launcher and the uninstaller are copies of the programs the manifest names in the
        // version folder just checked, never files from the archive's root.
        let named = |kind| {
            release
                .manifest
                .components
                .iter()
                .find(|component| component.kind == kind)
                .map(|component| version_folder.join(&component.relative_path))
        };
        let launcher =
            named(BuildComponentKind::Launcher).context("the release has no launcher")?;
        fs::copy(&launcher, staging.join(LAUNCHER))?;
        match named(BuildComponentKind::Setup) {
            Some(setup) => fs::copy(&setup, staging.join(UNINSTALLER)).map(|_| ())?,
            None => {
                fs::write(staging.join(UNINSTALLER), payload::program_without_release(program)?)?
            }
        }
        Replaced::rename(
            &staging.join(LAUNCHER),
            &root.join(LAUNCHER),
            root.join(format!(".replaced-{}.exe", nonce())),
        )?
        .keep();
        if staging.join(ICON).is_file() {
            Replaced::rename(
                &staging.join(ICON),
                &root.join(ICON),
                root.join(format!(".replaced-{}.ico", nonce())),
            )?
            .keep();
        }
        Replaced::rename(
            &staging.join(UNINSTALLER),
            &root.join(UNINSTALLER),
            root.join(format!(".replaced-{}.exe", nonce())),
        )?
        .keep();
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    result?;

    let state = match &plan.kind {
        Kind::Fresh => ActiveInstallState {
            active: release.version.clone(),
            release_descriptor: release.descriptor.clone(),
            previous: None,
            pending: None,
            last_known_good: Some(release.version.clone()),
        },
        Kind::Repair => read_install_state(root)?,
        Kind::Upgrade { .. } | Kind::Downgrade { .. } => {
            let mut state = read_install_state(root)?;
            state.pending = Some(release.version.clone());
            state
        }
    };
    write_install_state(root, &state)?;
    remove_unkept_versions(root, &state);

    let launcher = root.join(LAUNCHER);
    let icon = root.join(ICON);
    let shortcut = |folder: PathBuf| -> std::io::Result<()> {
        host.create_shortcut(&Shortcut {
            path: &folder.join(shortcut_name(&release.manifest.bundle_namespace)),
            target: &launcher,
            arguments: "",
            working_directory: root,
            description: "Aura",
            icon: icon.is_file().then_some(icon.as_path()),
        })
    };
    shortcut(host.start_menu()?)?;
    if options.desktop_shortcut {
        shortcut(host.desktop()?)?;
    }
    let uninstall_command = format!("\"{}\" --uninstall", root.join(UNINSTALLER).display());
    host.write_entry(
        &release.manifest.bundle_namespace,
        &UninstallEntry {
            key: "",
            display_name: "Aura",
            display_version: &release.version,
            publisher: PUBLISHER,
            install_location: root,
            display_icon: if icon.is_file() { &icon } else { &launcher },
            uninstall_command: &uninstall_command,
            estimated_size_kb: u32::try_from(folder_size(root) / 1024).unwrap_or(u32::MAX),
        },
    )?;
    Ok(())
}

/// Applies an update package to the installation at `root` while Aura runs from it: the package
/// must be a newer version of the same installation, and it waits as a trial for the next start.
pub fn apply_update(
    host: &dyn Host,
    program: &Path,
    root: &Path,
    package: &Path,
) -> Result<Release> {
    let installed = read_install_state(root)?;
    let mut archive = payload::open_package(package)?;
    let release = read_release(&mut archive)?;
    ensure!(
        release.manifest.bundle_namespace == installed.release_descriptor.bundle_namespace,
        "this update belongs to another Aura installation"
    );
    ensure!(
        compare(&release.version, &installed.active) == std::cmp::Ordering::Greater,
        "Aura {} is not newer than the installed {}",
        release.version,
        installed.active
    );
    let plan = Plan { root: root.to_path_buf(), kind: Kind::Upgrade { from: installed.active } };
    install(
        host,
        program,
        &mut archive,
        &release,
        &plan,
        &Options { desktop_shortcut: false, while_running: true },
        &mut |_, _| {},
    )?;
    Ok(release)
}

/// Lets the previous version wait as a trial for the next start.
pub fn roll_back(root: &Path) -> Result<String> {
    let state = read_install_state(root)?;
    let back =
        aura_launcher::roll_back(&state).context("there is no previous version to go back to")?;
    resolve_version(root, back.pending.as_deref().unwrap_or_default(), None)
        .context("the previous version's files are not complete")?;
    write_install_state(root, &back)?;
    Ok(back.pending.unwrap_or_default())
}
