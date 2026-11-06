//! The folders of the Skills usable now. The user keeps them for the model to read, so the file
//! tools read in them without asking, in every mode and also where they sit in Aura's own data.
//! A target is matched against a folder by identity: a link inside a Skill resolves to where it
//! points, which is not in the folder.
use super::*;
use aura_windows_platform::{NativePathInfo, inspect_existing_path};

impl AgentExecution {
    /// Whether `target` lies in the folder of a Skill usable now in `workspace`.
    pub(in crate::agent_execution) fn in_usable_skill_folder(
        &self,
        workspace: WorkspaceId,
        target: &NativePathInfo,
    ) -> bool {
        let Some(runtime) = self.skills.get() else { return false };
        let Ok(folders) =
            aura_extension_service::skills::loading::usable_folders(runtime, Some(workspace))
        else {
            return false;
        };
        folders.iter().any(|folder| {
            inspect_existing_path(folder, true)
                .is_ok_and(|root| root.is_directory && native_scope::within(&root, target))
        })
    }
}
