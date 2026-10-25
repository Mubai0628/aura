//! One command directory as it was read: usable, or with the reason it is not offered.
use aura_agent_domain::{CommandEntry, CommandKind, CommandSource};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedCommand {
    /// The directory name; for a valid command also its manifest name.
    pub name: String,
    pub source: CommandSource,
    pub location: PathBuf,
    pub title: Option<String>,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub template: Option<String>,
    /// `command.toml` exactly as stored, when it could be read.
    pub manifest: Option<String>,
    /// BLAKE3 over the manifest and template bytes; a change or removal is refused
    /// against an older revision.
    pub revision: Option<String>,
    pub issue: Option<String>,
}

impl LoadedCommand {
    pub(crate) fn unusable(
        name: String,
        source: CommandSource,
        location: PathBuf,
        issue: String,
    ) -> Self {
        Self {
            name,
            source,
            location,
            title: None,
            description: None,
            argument_hint: None,
            template: None,
            manifest: None,
            revision: None,
            issue: Some(issue),
        }
    }

    pub fn entry(&self, enabled: bool, issue: Option<String>) -> CommandEntry {
        CommandEntry {
            name: self.name.clone(),
            title: self.title.clone().unwrap_or_else(|| self.name.clone()),
            description: self.description.clone().unwrap_or_default(),
            kind: CommandKind::Prompt,
            source: self.source,
            plugin: None,
            argument_hint: self.argument_hint.clone(),
            template: self.template.clone(),
            enabled,
            // The command's own problem is the one to fix first.
            issue: self.issue.clone().or(issue),
            location: Some(aura_domain_common::display_path(&self.location)),
            revision: self.revision.clone(),
        }
    }
}
