//! The prompt commands the user turned off, kept in `commands.json` under `disabled`.
//! Only that key is read or written; anything else in the document is left exactly as it is.
use crate::{ConfigModule, SettingsFacade, SettingsFacadeError};
use std::collections::BTreeSet;

const DISABLED: &str = "disabled";

/// The keys of the commands the user turned off. When the stored list cannot be read,
/// nothing counts as off and `issue` says why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisabledCommands {
    pub keys: BTreeSet<String>,
    pub revision: Option<u64>,
    pub issue: Option<String>,
}

impl SettingsFacade {
    pub fn disabled_commands(&self) -> DisabledCommands {
        let Ok(snapshot) = self.snapshot(ConfigModule::Commands) else {
            return DisabledCommands {
                issue: Some("命令设置暂时无法读取，全部命令按启用处理".into()),
                ..DisabledCommands::default()
            };
        };
        let revision = Some(snapshot.document.revision);
        match snapshot.document.values.get(DISABLED) {
            None => DisabledCommands { revision, ..DisabledCommands::default() },
            Some(value) => match value.as_array().and_then(|items| {
                items.iter().map(|item| item.as_str().map(str::to_owned)).collect()
            }) {
                Some(keys) => DisabledCommands { keys, revision, issue: None },
                None => DisabledCommands {
                    keys: BTreeSet::new(),
                    revision,
                    issue: Some("commands.json 的 disabled 不是文字列表，已忽略".into()),
                },
            },
        }
    }

    /// Turns one command on or off at the document's current revision. A concurrent change
    /// is a conflict for the caller to retry, never a silent overwrite.
    pub fn set_command_enabled(&self, key: &str, enabled: bool) -> Result<(), SettingsFacadeError> {
        let current = self.disabled_commands();
        let revision = self.snapshot(ConfigModule::Commands)?.document.revision;
        let mut keys = current.keys;
        if enabled {
            keys.remove(key);
        } else {
            keys.insert(key.to_owned());
        }
        self.update(ConfigModule::Commands, revision, |values| {
            if keys.is_empty() {
                values.remove(DISABLED);
            } else {
                values.insert(DISABLED.to_owned(), serde_json::json!(keys));
            }
        })?;
        Ok(())
    }
}
