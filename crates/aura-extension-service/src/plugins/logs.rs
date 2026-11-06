//! What went wrong in a plugin's parts: its Hooks, MCP servers, panel and background service.
//! Kept in Aura's plugin folder, one bounded file per plugin, for the plugin page and
//! `plugins.logs`; the MCP servers' standard error is read live. Secret values the plugin was
//! given are removed before storage and also from legacy entries when read.
use super::runtime::PluginRuntime;
use aura_agent_domain::{PluginLogEntry, PluginLogSource};
use aura_domain_common::UtcTimestamp;
use std::io::Write;
use std::path::PathBuf;

const LOGS: &str = "logs";
/// One file reaches this, then it becomes the older file and a new one starts.
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_TEXT_CHARS: usize = 4000;
/// The most standard error lines of each server that are read.
const MAX_SERVER_LINES: usize = 40;
const HIDDEN: &str = "••••";
const WITHHELD: &str = "日志内容已隐藏：无法确认插件的配置或保密值";
const MAX_REMEMBERED_BYTES: usize = 64 * 1024;

impl PluginRuntime {
    fn log_file(&self, name: &str, older: bool) -> PathBuf {
        self.root().join(LOGS).join(if older {
            format!("{name}.1.jsonl")
        } else {
            format!("{name}.jsonl")
        })
    }

    /// Keeps one entry. A failure to keep it is not the plugin's problem and is not reported.
    pub fn log(&self, name: &str, source: PluginLogSource, part: &str, text: &str) {
        let registry = self.settings().plugin_registry();
        let secrets = if registry.issue.is_some() {
            Err(anyhow::anyhow!("plugin configuration unavailable"))
        } else {
            self.secret_values(name, registry.plugin(name))
        };
        let (text, part) = match secrets {
            Ok(secrets) => (hidden(text, &secrets), hidden(part, &secrets)),
            Err(_) => (WITHHELD.into(), WITHHELD.into()),
        };
        let text: String = text.chars().take(MAX_TEXT_CHARS).collect();
        let entry = PluginLogEntry { at: UtcTimestamp::now(), source, part, text };
        let Ok(mut line) = serde_json::to_vec(&entry) else { return };
        line.push(b'\n');
        let Ok(_writing) = self.changing_logs.lock() else { return };
        let file = self.log_file(name, false);
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        if std::fs::metadata(&file).is_ok_and(|m| m.len() + line.len() as u64 > MAX_FILE_BYTES) {
            let _ = std::fs::rename(&file, self.log_file(name, true));
        }
        if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(&file) {
            let _ = out.write_all(&line);
        }
    }

    pub(crate) fn remove_logs(&self, name: &str) {
        for older in [false, true] {
            let _ = std::fs::remove_file(self.log_file(name, older));
        }
    }

    /// The newest `limit` entries, oldest first, and whether older ones were left out: what was
    /// kept, then each of the plugin's servers' standard error as it is now.
    pub fn recent_logs(
        &self,
        name: &str,
        limit: usize,
    ) -> anyhow::Result<(Vec<PluginLogEntry>, bool)> {
        let (_, located) = self.locate(name)?;
        let mut entries: Vec<PluginLogEntry> = [true, false]
            .into_iter()
            .filter_map(|older| std::fs::read_to_string(self.log_file(name, older)).ok())
            .flat_map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str::<PluginLogEntry>(line).ok())
                    .collect::<Vec<_>>()
            })
            .collect();
        if let (Some(mcp), Ok(package)) = (&self.mcp, &located.scanned) {
            let now = UtcTimestamp::now();
            for server in &package.servers {
                let lines = mcp.stderr(&server.id);
                let start = lines.len().saturating_sub(MAX_SERVER_LINES);
                entries.extend(lines[start..].iter().map(|line| PluginLogEntry {
                    at: now,
                    source: PluginLogSource::Mcp,
                    part: server.name.clone(),
                    text: line.chars().take(MAX_TEXT_CHARS).collect(),
                }));
            }
        }
        let secrets = self.secret_values(name, located.setting.as_ref());
        for entry in &mut entries {
            match &secrets {
                Ok(secrets) => {
                    entry.text = hidden(&entry.text, secrets);
                    entry.part = hidden(&entry.part, secrets);
                }
                Err(_) => {
                    entry.text = WITHHELD.into();
                    entry.part = WITHHELD.into();
                }
            }
        }
        let truncated = entries.len() > limit;
        let skip = entries.len().saturating_sub(limit);
        Ok((entries.split_off(skip), truncated))
    }

    /// Keeps old values available for masking while a replaced service may still write output.
    pub(crate) fn remember_log_secret(&self, name: &str, value: &str) {
        if value.is_empty() {
            return;
        }
        let mut known = self.log_secrets.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let retained = known.entry(name.to_owned()).or_insert_with(|| Some(Vec::new()));
        let Some(values) = retained else { return };
        if values.iter().any(|known| known.as_str() == value) {
            return;
        }
        if values.iter().map(|value| value.len()).sum::<usize>() + value.len()
            > MAX_REMEMBERED_BYTES
        {
            // Withhold future logs rather than discard an old value and expose late output.
            *retained = None;
        } else {
            values.push(zeroize::Zeroizing::new(value.to_owned()));
        }
    }

    fn secret_values(
        &self,
        name: &str,
        setting: Option<&aura_plugins::PluginSetting>,
    ) -> anyhow::Result<Vec<String>> {
        let mut values: Vec<String> = {
            let known = self.log_secrets.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match known.get(name) {
                Some(Some(values)) => values.iter().map(|value| value.to_string()).collect(),
                Some(None) => anyhow::bail!("plugin log protection capacity reached"),
                None => Vec::new(),
            }
        };
        let Some(setting) = setting else { return Ok(values) };
        let servers =
            setting.secrets.iter().map(|secret| (secret.server.clone(), secret.secret_id));
        let own = setting
            .values
            .secrets
            .iter()
            .map(|(key, id)| (super::service::secret_owner(&setting.name, key), *id));
        for (owner, id) in servers.chain(own) {
            let mcp = self
                .mcp
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("plugin credential store unavailable"))?;
            let (_, value) = mcp
                .secrets()
                .value(&owner, id)
                .map_err(|_| anyhow::anyhow!("plugin credential unavailable"))?;
            let text = std::str::from_utf8(value.expose())
                .map_err(|_| anyhow::anyhow!("plugin credential is not text"))?;
            values.push(text.to_owned());
        }
        // A line-framed source may split these values before this layer can mask them.
        anyhow::ensure!(
            !values.iter().any(|value| value.contains(['\r', '\n'])),
            "plugin credential spans log lines"
        );
        Ok(values)
    }
}

/// Masks nonempty values in the original input only, longest first, without rescanning masks.
pub(crate) fn hidden(text: &str, secrets: &[String]) -> String {
    let mut secrets: Vec<&str> =
        secrets.iter().map(String::as_str).filter(|secret| !secret.is_empty()).collect();
    secrets.sort_unstable_by_key(|secret| std::cmp::Reverse(secret.len()));
    secrets.dedup();
    let mut remaining = text;
    let mut result = String::new();
    while !remaining.is_empty() {
        if let Some(secret) = secrets.iter().find(|secret| remaining.starts_with(**secret)) {
            result.push_str(HIDDEN);
            remaining = &remaining[secret.len()..];
        } else if let Some(character) = remaining.chars().next() {
            result.push(character);
            remaining = &remaining[character.len_utf8()..];
        }
    }
    result
}
