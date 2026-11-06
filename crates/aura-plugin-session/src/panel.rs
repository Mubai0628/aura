//! What Main is given to show a plugin's panel, from the plugin as it is trusted now.
use aura_agent_domain::PluginSessionPanel;
use aura_plugins::PluginPackage;

/// The panel of a usable plugin, or why it cannot be shown.
pub fn panel_site(package: &PluginPackage) -> anyhow::Result<PluginSessionPanel> {
    let name = &package.manifest.name;
    let Some(panel) = &package.manifest.declared.panel else {
        anyhow::bail!("plugin_session.panel_missing: 插件 {name} 没有面板");
    };
    anyhow::ensure!(
        package.panel_problems.is_empty(),
        "plugin_session.panel_unusable: 插件 {name} 的面板现在不能用：{}",
        package.panel_problems.join("；")
    );
    Ok(PluginSessionPanel {
        title: panel.title.clone(),
        folder: aura_domain_common::display_path(&package.root),
        entry: panel.entry.clone(),
        network: panel.network.clone(),
        capabilities: package
            .manifest
            .declared
            .capabilities
            .iter()
            .map(|capability| capability.name().to_owned())
            .collect(),
    })
}
