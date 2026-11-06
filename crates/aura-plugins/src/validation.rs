//! Checking a plugin folder before it is installed, as the model that writes plugins sees it:
//! every problem with the part it is in, whether the plugin can be installed at all, and what it
//! brings that can be used.
use crate::{PluginCounts, PluginPackage, PluginPackageError, scan_plugin};
use aura_skills::FileHashCache;
use std::path::Path;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PluginValidation {
    pub name: Option<String>,
    pub version: Option<String>,
    /// False when the manifest or the layout cannot be used: then nothing of it can be installed.
    /// A part with a problem is shown and not offered; the rest of the plugin still works.
    pub installable: bool,
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    pub brings: PluginCounts,
}

/// Reads and checks the plugin in `root`. Nothing is run.
#[must_use]
pub fn validate_plugin(root: &Path, cache: &FileHashCache) -> PluginValidation {
    match scan_plugin(root, cache) {
        Ok(package) => validation(&package),
        Err(PluginPackageError::Invalid(problems)) => {
            PluginValidation { problems, ..PluginValidation::default() }
        }
        Err(error) => {
            PluginValidation { problems: vec![error.to_string()], ..PluginValidation::default() }
        }
    }
}

/// The problems and warnings of a plugin that was read, each named by its part.
#[must_use]
pub fn validation(package: &PluginPackage) -> PluginValidation {
    let mut problems = Vec::new();
    let mut warnings = package.warnings.clone();
    for item in package.skill_items() {
        for problem in item.problems {
            problems.push(format!("Skill skills/{}：{problem}", item.relative));
        }
    }
    for item in package.command_items() {
        if let Some(problem) = item.problem {
            problems.push(format!("命令 /{}：{problem}", item.name));
        }
        for warning in item.warnings {
            warnings.push(format!("命令 /{}：{warning}", item.name));
        }
    }
    for item in package.hook_items() {
        for problem in item.problems {
            problems.push(format!("Hook hooks/{}：{problem}", item.folder));
        }
    }
    for server in &package.servers {
        for problem in &server.problems {
            problems.push(format!("MCP 服务器 {}：{problem}", server.name));
        }
    }
    problems.extend(package.panel_problems.iter().map(|problem| format!("面板：{problem}")));
    problems.extend(package.service_problems.iter().map(|problem| format!("后台服务：{problem}")));
    if let Some(Err(theme)) = &package.theme {
        problems.extend(theme.iter().map(|problem| format!("主题：{problem}")));
    }
    warnings.extend(package.unsupported.iter().map(|part| format!("不使用：{part}")));
    PluginValidation {
        name: Some(package.manifest.name.clone()),
        version: Some(package.manifest.version.clone()),
        installable: true,
        problems,
        warnings,
        brings: package.counts(),
    }
}
