//! What installing and uninstalling change outside the install folder, behind one trait so the
//! tests run the same steps against a folder of their own.
use std::{
    env, io,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::fs;

use aura_chrome_domain::protocol::native_host_name;
use aura_runtime_contract::BundleNamespace;
use aura_windows_platform::installer::{self, RunningProgram, Shortcut, UninstallEntry};

pub trait Host {
    /// The folder a new installation goes in when the user picks none.
    fn default_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf>;
    /// Where an existing installation of this namespace is, from its Settings > Apps entry.
    fn installed_root(&self, namespace: &BundleNamespace) -> io::Result<Option<PathBuf>>;
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
    /// The installation's data folder (`%LOCALAPPDATA%\Aura\namespaces\<hash>`).
    fn data_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf>;
    /// The installation's settings folder (`%APPDATA%\Aura\namespaces\<hash>`).
    fn config_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf>;
    fn credentials(&self, namespace: &BundleNamespace) -> io::Result<Vec<String>>;
    fn delete_credentials(&self, namespace: &BundleNamespace) -> io::Result<usize>;
    /// Removes Chrome's registration of the installation's relay; `false` when there was none.
    fn remove_chrome_host(&self, namespace: &BundleNamespace) -> io::Result<bool>;
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

fn local_app_data() -> io::Result<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))
}

fn app_data() -> io::Result<PathBuf> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "APPDATA is not set"))
}

/// The user's own Windows session.
pub struct SystemHost;

impl Host for SystemHost {
    fn default_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf> {
        Ok(local_app_data()?.join("Programs").join(folder_name(namespace)))
    }

    fn installed_root(&self, namespace: &BundleNamespace) -> io::Result<Option<PathBuf>> {
        installer::read_uninstall_location(&entry_key(namespace))
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

    fn data_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf> {
        Ok(local_app_data()?.join("Aura").join("namespaces").join(namespace.object_name_hash()))
    }

    fn config_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf> {
        Ok(app_data()?.join("Aura").join("namespaces").join(namespace.object_name_hash()))
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

    fn data_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf> {
        Ok(self.root.join("LocalAppData/Aura/namespaces").join(namespace.object_name_hash()))
    }

    fn config_root(&self, namespace: &BundleNamespace) -> io::Result<PathBuf> {
        Ok(self.root.join("AppData/Aura/namespaces").join(namespace.object_name_hash()))
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
}
