//! The registry as it is now: what the settings page shows, and which Hooks hear an event. A
//! ready plugin's Hooks come after the user's own, each at the fingerprint it had when the
//! plugin was trusted and under the plugin's limits.
use aura_config::{HookRegistration, HookRegistry};
use aura_hooks::{
    HOOK_MANIFEST_FILE, HookDefinition, HookEntryState, HookEntryView, HookEvent, parse_manifest,
    scan_package,
};
use std::path::Path;

/// A registered, enabled and trusted Hook that hears the event, by its manifest. The full
/// content check happens when it runs, while its files are held.
pub(crate) struct Candidate {
    pub path: String,
    pub trusted_fingerprint: String,
    pub definition: HookDefinition,
    /// A plugin's Hook runs under the plugin's limits.
    pub limits: Option<aura_windows_platform::JobLimits>,
}

/// Every entry with its live state, in execution order.
pub(crate) fn entry_views(registry: &HookRegistry) -> Vec<HookEntryView> {
    registry.entries.iter().map(view).collect()
}

fn view(entry: &HookRegistration) -> HookEntryView {
    match scan_package(Path::new(&entry.path)) {
        Err(error) => HookEntryView {
            path: entry.path.clone(),
            plugin: None,
            enabled: entry.enabled,
            state: HookEntryState::Invalid,
            definition: None,
            fingerprint: None,
            trusted_fingerprint: entry.trusted_fingerprint.clone(),
            problems: vec![error.to_string()],
        },
        Ok(package) => HookEntryView {
            path: entry.path.clone(),
            plugin: None,
            enabled: entry.enabled,
            state: match &entry.trusted_fingerprint {
                None => HookEntryState::NotTrusted,
                Some(trusted) if *trusted != package.fingerprint => HookEntryState::ContentChanged,
                Some(_) if !entry.enabled => HookEntryState::Disabled,
                Some(_) => HookEntryState::Ready,
            },
            definition: Some(package.definition),
            fingerprint: Some(package.fingerprint),
            trusted_fingerprint: entry.trusted_fingerprint.clone(),
            problems: Vec::new(),
        },
    }
}

/// The Hooks that hear `event` (for a `tool.*` event, this tool), in registry order. Only
/// the manifest is read here; an entry whose manifest cannot be read hears nothing.
pub(crate) fn candidates(
    registry: &HookRegistry,
    event: HookEvent,
    tool: Option<&str>,
) -> Vec<Candidate> {
    if registry.issue.is_some() {
        return Vec::new();
    }
    registry
        .entries
        .iter()
        .filter(|entry| entry.enabled)
        .filter_map(|entry| {
            let trusted = entry.trusted_fingerprint.clone()?;
            let text = read_manifest(Path::new(&entry.path))?;
            let definition = parse_manifest(&text).ok()?;
            let heard = definition.hears(event)
                && (!event.tool_scoped() || tool.is_some_and(|t| definition.matches_tool(t)));
            heard.then(|| Candidate {
                path: entry.path.clone(),
                trusted_fingerprint: trusted,
                definition,
                limits: None,
            })
        })
        .collect()
}

/// The ready plugins' Hooks that hear `event`, after the user's own: plugins by name, each
/// plugin's Hooks by folder.
pub(crate) fn plugin_candidates(
    plugins: &crate::plugins::PluginRuntime,
    event: HookEvent,
    tool: Option<&str>,
) -> Vec<Candidate> {
    let mut found = Vec::new();
    for plugin in plugins.ready().iter() {
        let switched_off = &plugin.setting.changes.switched_off.hooks;
        for hook in plugin.package.hooks.iter().filter(|hook| !switched_off.contains(&hook.folder))
        {
            let (Ok(package), Some(trusted)) =
                (&hook.package, plugin.setting.trusted_parts.hooks.get(&hook.folder))
            else {
                continue;
            };
            let definition = &package.definition;
            if definition.hears(event)
                && (!event.tool_scoped() || tool.is_some_and(|t| definition.matches_tool(t)))
            {
                found.push(Candidate {
                    path: package.root.display().to_string(),
                    trusted_fingerprint: trusted.clone(),
                    definition: definition.clone(),
                    limits: Some(plugin.job_limits()),
                });
            }
        }
    }
    found
}

/// The ready plugins' Hooks as the settings page shows them, each read now.
pub(crate) fn plugin_entry_views(plugins: &crate::plugins::PluginRuntime) -> Vec<HookEntryView> {
    let mut views = Vec::new();
    for plugin in plugins.ready().iter() {
        for hook in &plugin.package.hooks {
            let root = plugin.package.root.join("hooks").join(&hook.folder);
            let trusted = plugin.setting.trusted_parts.hooks.get(&hook.folder).cloned();
            let mut entry = view(&HookRegistration {
                path: root.display().to_string(),
                enabled: !plugin.setting.changes.switched_off.hooks.contains(&hook.folder),
                trusted_fingerprint: trusted,
            });
            entry.plugin = Some(plugin.package.manifest.name.clone());
            views.push(entry);
        }
    }
    views
}

fn read_manifest(root: &Path) -> Option<String> {
    let path = root.join(HOOK_MANIFEST_FILE);
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    (metadata.is_file() && metadata.len() <= 16 * 1024)
        .then(|| std::fs::read_to_string(path).ok())
        .flatten()
}
