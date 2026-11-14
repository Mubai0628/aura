//! What installing and uninstalling change outside the install folder, behind one trait so the
//! tests run the same steps against a folder of their own.
use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::fs;

use aura_chrome_domain::protocol::native_host_name;
use aura_data_location::Roots;
use aura_runtime_contract::BundleNamespace;
use aura_windows_platform::installer::{self, RunningProgram, Shortcut, UninstallEntry};

pub trait Host {
    /// The folder a new installation goes in when the user picks none.
    fn default_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf>;
    /// Where an existing installation of this namespace is, from its Settings > Apps entry.
    fn installed_root(&self, namespace: &BundleNamespace) -> io::Result<Option<PathBuf>>;
    /// The folders the Settings > Apps entries of every Aura installation name; `None` for an
    /// entry that names none.
    fn installed_folders(&self) -> io::Result<Vec<Option<PathBuf>>>;
    fn write_entry(
        &self,
        namespace: &BundleNamespace,
        entry: &UninstallEntry<'_>,
    ) -> io::Result<()>;
    fn remove_entry(&self, namespace: &BundleNamespace) -> io::Result<bool>;
    fn shortcut_folders(&self) -> io::Result<Vec<PathBuf>>;
    fn desktop(&self) -> io::Result<PathBuf>;
    fn start_menu(&self) -> io::Result<PathBuf>;
    fn create_shortcut(&self, shortcut: &Shortcut<'_>) -> io::Result<()>;
    fn running_in(&self, root: &Path) -> io::Result<Vec<RunningProgram>>;
    /// Ends a program `running_in` found, while its process still runs that executable.
    fn stop(&self, program: &RunningProgram) -> io::Result<()>;
    /// The user's folders the installation's locations are worked out from.
    fn roots(&self) -> io::Result<Roots>;
    fn credentials(&self, namespace: &BundleNamespace) -> io::Result<Vec<String>>;
    fn delete_credentials(&self, namespace: &BundleNamespace) -> io::Result<usize>;
    /// Removes Chrome's registration of the installation's relay; `false` when there was none.
    fn remove_chrome_host(&self, namespace: &BundleNamespace) -> io::Result<bool>;
    /// Removes the name Windows heads the installation's notifications with.
    fn remove_notification_identity(&self, namespace: &BundleNamespace) -> io::Result<bool>;
    /// The folder of the shortcuts pinned to the taskbar.
    fn taskbar_pins(&self) -> io::Result<PathBuf>;
    /// The manifest Chrome finds for the installation's relay, when Chrome control registered one.
    fn chrome_host_manifest(&self, namespace: &BundleNamespace) -> io::Result<Option<PathBuf>>;
    /// Points the notification name's icon into `to` when it is inside `from`.
    fn relocate_notification_icon(
        &self,
        namespace: &BundleNamespace,
        from: &Path,
        to: &Path,
    ) -> io::Result<bool>;
}

/// The entry key in Settings > Apps: `Aura` for the released product, its own key for a test
/// release, so the two never meet.
pub fn entry_key(namespace: &BundleNamespace) -> String {
    match namespace.as_str().strip_prefix("aura.desktop.test.") {
        Some(run) => format!("Aura-test-{run}"),
        None => "Aura".to_owned(),
    }
}

/// The folder name under `%LOCALAPPDATA%\Programs`.
pub fn folder_name(namespace: &BundleNamespace) -> String {
    match namespace.as_str().strip_prefix("aura.desktop.test.") {
        Some(run) => format!("Aura Test {run}"),
        None => "Aura".to_owned(),
    }
}

/// The shortcut file name, `Aura.lnk` for the released product.
pub fn shortcut_name(namespace: &BundleNamespace) -> String {
    format!("{}.lnk", folder_name(namespace))
}

pub fn credential_prefix(namespace: &BundleNamespace) -> String {
    format!("Aura/{}/", namespace.object_name_hash())
}

/// The user's own Windows session.
pub struct SystemHost;

impl Host for SystemHost {
    fn default_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf> {
        Ok(self.roots()?.local.join("Programs").join(folder_name(namespace)))
    }

    fn installed_root(&self, namespace: &BundleNamespace) -> io::Result<Option<PathBuf>> {
        installer::read_uninstall_location(&entry_key(namespace))
    }

    fn installed_folders(&self) -> io::Result<Vec<Option<PathBuf>>> {
        Ok(installer::aura_uninstall_entries()?.into_iter().map(|(_, folder)| folder).collect())
    }

    fn write_entry(
        &self,
        namespace: &BundleNamespace,
        entry: &UninstallEntry<'_>,
    ) -> io::Result<()> {
        let key = entry_key(namespace);
        installer::write_uninstall_entry(&UninstallEntry { key: &key, ..*entry })
    }

    fn remove_entry(&self, namespace: &BundleNamespace) -> io::Result<bool> {
        installer::remove_uninstall_entry(&entry_key(namespace))
    }

    fn shortcut_folders(&self) -> io::Result<Vec<PathBuf>> {
        Ok(vec![self.start_menu()?, self.desktop()?])
    }

    fn desktop(&self) -> io::Result<PathBuf> {
        installer::desktop_folder()
    }

    fn start_menu(&self) -> io::Result<PathBuf> {
        installer::start_menu_programs()
    }

    fn create_shortcut(&self, shortcut: &Shortcut<'_>) -> io::Result<()> {
        installer::create_shortcut(shortcut)
    }

    fn running_in(&self, root: &Path) -> io::Result<Vec<RunningProgram>> {
        if !root.exists() {
            return Ok(Vec::new());
        }
        installer::programs_running_in(root)
    }

    fn stop(&self, program: &RunningProgram) -> io::Result<()> {
        installer::stop_program(program)
    }

    fn roots(&self) -> io::Result<Roots> {
        Roots::from_system().map_err(io::Error::other)
    }

    fn credentials(&self, namespace: &BundleNamespace) -> io::Result<Vec<String>> {
        installer::installation_credentials(&credential_prefix(namespace))
    }

    fn delete_credentials(&self, namespace: &BundleNamespace) -> io::Result<usize> {
        installer::delete_installation_credentials(&credential_prefix(namespace))
    }

    fn remove_chrome_host(&self, namespace: &BundleNamespace) -> io::Result<bool> {
        installer::remove_chrome_native_host(&native_host_name(namespace.as_str()))
    }

    fn remove_notification_identity(&self, namespace: &BundleNamespace) -> io::Result<bool> {
        installer::remove_notification_identity(namespace.as_str())
    }

    fn taskbar_pins(&self) -> io::Result<PathBuf> {
        Ok(self
            .roots()?
            .roaming
            .join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar"))
    }

    fn chrome_host_manifest(&self, namespace: &BundleNamespace) -> io::Result<Option<PathBuf>> {
        installer::read_chrome_native_host(&native_host_name(namespace.as_str()))
    }

    fn relocate_notification_icon(
        &self,
        namespace: &BundleNamespace,
        from: &Path,
        to: &Path,
    ) -> io::Result<bool> {
        installer::relocate_notification_icon(namespace.as_str(), from, to)
    }
}

/// A host inside one folder: entries are JSON files and shortcuts land in its own folders.
#[cfg(test)]
pub struct FolderHost {
    pub root: PathBuf,
}

#[cfg(test)]
impl FolderHost {
    fn entry_path(&self, namespace: &BundleNamespace) -> PathBuf {
        self.root.join("entries").join(format!("{}.json", entry_key(namespace)))
    }

    pub fn entry(&self, namespace: &BundleNamespace) -> Option<serde_json::Value> {
        fs::read(self.entry_path(namespace))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    }

    fn chrome_host_path(&self, name: &str) -> PathBuf {
        self.root.join("chrome-hosts").join(name)
    }

    /// Registers `name` for Chrome the way Core does when Chrome control is turned on.
    pub fn register_chrome_host(&self, name: &str, manifest: &Path) {
        let path = self.chrome_host_path(name);
        fs::create_dir_all(path.parent().unwrap_or(&self.root)).expect("chrome hosts folder");
        fs::write(path, manifest.to_string_lossy().as_bytes()).expect("chrome host");
    }

    pub fn chrome_host(&self, name: &str) -> Option<PathBuf> {
        fs::read_to_string(self.chrome_host_path(name)).ok().map(PathBuf::from)
    }
}

#[cfg(test)]
impl Host for FolderHost {
    fn default_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf> {
        Ok(self.root.join("Programs").join(folder_name(namespace)))
    }

    fn installed_root(&self, namespace: &BundleNamespace) -> io::Result<Option<PathBuf>> {
        Ok(self
            .entry(namespace)
            .and_then(|entry| entry["InstallLocation"].as_str().map(PathBuf::from)))
    }

    fn installed_folders(&self) -> io::Result<Vec<Option<PathBuf>>> {
        let mut folders = Vec::new();
        for entry in fs::read_dir(self.root.join("entries")).into_iter().flatten() {
            let bytes = fs::read(entry?.path())?;
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(io::Error::other)?;
            folders.push(value["InstallLocation"].as_str().map(PathBuf::from));
        }
        Ok(folders)
    }

    fn write_entry(
        &self,
        namespace: &BundleNamespace,
        entry: &UninstallEntry<'_>,
    ) -> io::Result<()> {
        let path = self.entry_path(namespace);
        fs::create_dir_all(path.parent().unwrap_or(&self.root))?;
        let value = serde_json::json!({
            "DisplayName": entry.display_name,
            "DisplayVersion": entry.display_version,
            "Publisher": entry.publisher,
            "InstallLocation": entry.install_location,
            "DisplayIcon": entry.display_icon,
            "UninstallString": entry.uninstall_command,
            "EstimatedSize": entry.estimated_size_kb,
        });
        fs::write(path, value.to_string())
    }

    fn remove_entry(&self, namespace: &BundleNamespace) -> io::Result<bool> {
        match fs::remove_file(self.entry_path(namespace)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn shortcut_folders(&self) -> io::Result<Vec<PathBuf>> {
        Ok(vec![self.start_menu()?, self.desktop()?])
    }

    fn desktop(&self) -> io::Result<PathBuf> {
        Ok(self.root.join("Desktop"))
    }

    fn start_menu(&self) -> io::Result<PathBuf> {
        Ok(self.root.join("Start Menu"))
    }

    fn create_shortcut(&self, shortcut: &Shortcut<'_>) -> io::Result<()> {
        installer::create_shortcut(shortcut)
    }

    fn running_in(&self, root: &Path) -> io::Result<Vec<RunningProgram>> {
        if !root.exists() {
            return Ok(Vec::new());
        }
        installer::programs_running_in(root)
    }

    fn stop(&self, program: &RunningProgram) -> io::Result<()> {
        installer::stop_program(program)
    }

    fn roots(&self) -> io::Result<Roots> {
        Ok(Roots {
            local: self.root.join("LocalAppData"),
            roaming: self.root.join("AppData"),
            profile: self.root.join("Profile"),
            known_local: self.root.join("LocalAppData"),
        })
    }

    fn credentials(&self, _namespace: &BundleNamespace) -> io::Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn delete_credentials(&self, _namespace: &BundleNamespace) -> io::Result<usize> {
        Ok(0)
    }

    fn remove_chrome_host(&self, namespace: &BundleNamespace) -> io::Result<bool> {
        match fs::remove_file(self.chrome_host_path(&native_host_name(namespace.as_str()))) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn remove_notification_identity(&self, _namespace: &BundleNamespace) -> io::Result<bool> {
        Ok(false)
    }

    fn taskbar_pins(&self) -> io::Result<PathBuf> {
        Ok(self.root.join("TaskBar"))
    }

    fn chrome_host_manifest(&self, namespace: &BundleNamespace) -> io::Result<Option<PathBuf>> {
        Ok(self.chrome_host(&native_host_name(namespace.as_str())))
    }

    fn relocate_notification_icon(
        &self,
        _namespace: &BundleNamespace,
        _from: &Path,
        _to: &Path,
    ) -> io::Result<bool> {
        Ok(false)
    }
}
