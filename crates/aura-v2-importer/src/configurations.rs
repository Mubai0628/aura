use crate::{ImportDisposition as D, StoreResult, item, source::PreparedImport};
use aura_config::{ConfigModule, ModuleConfigDocument, ModuleConfigValidator};
use aura_model_domain::ModelRegistryConfig;
use rusqlite::Connection;

pub(crate) const SETTINGS_MODULES: &[ConfigModule] = &[
    ConfigModule::Ui,
    ConfigModule::Prompt,
    ConfigModule::Browser,
    ConfigModule::Memory,
    ConfigModule::Diagnostics,
    ConfigModule::Update,
];
pub(crate) fn is_config_path(name: &str) -> bool {
    matches!(name, "models.json" | "aura.toml" | "memory-sources.json")
        || SETTINGS_MODULES.iter().any(|m| m.relative_path() == name)
}

pub(crate) fn prepare(plan: &mut PreparedImport, legacy: &Connection) -> StoreResult<()> {
    let mut registry = ModelRegistryConfig::default();
    if let Some(path) = &plan.input.models {
        let decoded = aura_config::parse_document(ConfigModule::Models, &plan.source_files[path]);
        let candidate = decoded.ok().filter(|doc| {
            aura_model_config::ModelConfigValidator.validate(ConfigModule::Models, doc).is_ok()
        });
        if let Some(mut document) = candidate {
            registry = if document.values.is_empty() {
                ModelRegistryConfig::default()
            } else {
                serde_json::from_value(serde_json::to_value(&document.values)?)?
            };
            for endpoint in registry.endpoints.values_mut() {
                endpoint.compatibility = None;
                item(
                    &mut plan.report,
                    "endpoint",
                    &endpoint.id.to_string(),
                    D::Imported,
                    "configuration_without_credential_or_learning",
                );
                if endpoint.auth_scheme != aura_model_domain::EndpointAuthScheme::None {
                    item(
                        &mut plan.report,
                        "endpoint_credential",
                        &endpoint.id.to_string(),
                        D::RequiresUserAction,
                        "credential_rebind_required",
                    );
                }
            }
            for provider in registry.providers.values() {
                item(
                    &mut plan.report,
                    "provider",
                    &provider.id.to_string(),
                    D::Imported,
                    "validated_configuration",
                );
            }
            for model in registry.models.values_mut() {
                // Runtime correction refers to discarded legacy evidence.
                model.context.runtime_corrected_window_tokens = None;
                model.context.runtime_correction_basis = None;
                model.context.runtime_effective_window_tokens =
                    model.context.effective_window_tokens(false);
                item(
                    &mut plan.report,
                    "profile",
                    &model.id.to_string(),
                    D::Imported,
                    "validated_configuration",
                );
            }
            aura_model_config::validate_model_registry(&registry)
                .map_err(|_| crate::Error::Invalid("normalized model configuration"))?;
            document.values = serde_json::from_value(serde_json::to_value(&registry)?)?;
            document.revision = 1;
            plan.configurations.insert(
                "models.json".into(),
                aura_config::encode_document(ConfigModule::Models, &document)
                    .map_err(|_| crate::Error::Invalid("model document encoding"))?,
            );
        } else {
            item(
                &mut plan.report,
                "models",
                "models",
                D::Invalid,
                "model_schema_or_secret_rejected",
            );
        }
    }
    if let Some(path) = &plan.input.settings {
        let decoded = aura_config::parse_document(ConfigModule::Aura, &plan.source_files[path]);
        if let Ok(mut doc) = decoded
            && aura_config::validate_base_document(&doc).is_ok()
            && !credential_reference(&serde_json::to_value(&doc)?)
        {
            doc.revision = 1;
            plan.configurations.insert(
                "aura.toml".into(),
                aura_config::encode_document(ConfigModule::Aura, &doc)
                    .map_err(|_| crate::Error::Invalid("settings encoding"))?,
            );
            item(&mut plan.report, "settings", "aura", D::Imported, "validated_nonsecret_settings");
        } else {
            item(
                &mut plan.report,
                "settings",
                "aura",
                D::Invalid,
                "settings_schema_or_credential_rejected",
            );
        }
    }
    if let Some(root) = &plan.input.config_root {
        for module in SETTINGS_MODULES {
            let Some(bytes) = plan.source_files.get(&root.join(module.relative_path())) else {
                continue;
            };
            match aura_config::parse_document(*module, bytes) {
                Ok(mut doc)
                    if aura_config::validate_base_document(&doc).is_ok()
                        && !credential_reference(&serde_json::to_value(&doc)?) =>
                {
                    doc.revision = 1;
                    plan.configurations.insert(
                        module.relative_path().into(),
                        aura_config::encode_document(*module, &doc)
                            .map_err(|_| crate::Error::Invalid("settings encoding"))?,
                    );
                    item(
                        &mut plan.report,
                        "settings",
                        module.key(),
                        D::Imported,
                        "validated_nonsecret_settings",
                    );
                }
                _ => item(
                    &mut plan.report,
                    "settings",
                    module.key(),
                    D::Invalid,
                    "settings_schema_or_credential_rejected",
                ),
            }
        }
        for module in [
            ConfigModule::Tools,
            ConfigModule::Hooks,
            ConfigModule::Mcp,
            ConfigModule::Skills,
            ConfigModule::Commands,
            ConfigModule::Plugins,
            ConfigModule::Security,
        ] {
            if root.join(module.relative_path()).exists() {
                item(
                    &mut plan.report,
                    "configuration",
                    module.key(),
                    D::Skipped,
                    "extension_or_security_state_requires_later_stage",
                );
            }
        }
    }
    // Missing files get valid empty documents in the new, isolated namespace.
    for (name, module) in [("models.json", ConfigModule::Models), ("aura.toml", ConfigModule::Aura)]
    {
        if !plan.configurations.contains_key(name) {
            plan.configurations.insert(
                name.into(),
                aura_config::encode_document(module, &ModuleConfigDocument::empty())
                    .map_err(|_| crate::Error::Invalid("empty config"))?,
            );
        }
    }
    let mut q=legacy.prepare("SELECT session_id,model_profile_id FROM session_model_bindings WHERE model_profile_id IS NOT NULL ORDER BY session_id")?;
    for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (session, profile) = row?;
        if let (Ok(s), Ok(p)) = (session.parse(), profile.parse())
            && plan.sessions.iter().any(|v| v.id == s)
            && registry.models.get(&p).is_some_and(|m| m.enabled)
        {
            plan.bindings.push((s, p));
            item(
                &mut plan.report,
                "session_binding",
                &session,
                D::Imported,
                "profile_exists_and_enabled",
            );
        } else {
            item(
                &mut plan.report,
                "session_binding",
                &session,
                D::RequiresUserAction,
                "session_or_profile_unavailable",
            );
        }
    }
    if let Some(path) = &plan.input.memory_vault {
        match aura_windows_platform::inspect_existing_path(path, false) {
            Ok(info) if path.is_absolute() && info.is_directory && !info.is_reparse_point => {
                let reference = serde_json::json!({"schema_version":1,"root":info.final_path,"volume_serial_number":info.identity.volume_serial_number.to_string(),"file_id":info.identity.file_id.iter().map(|b|format!("{b:02x}")).collect::<String>(),"index_state":"rebuild_required"});
                plan.configurations
                    .insert("memory-sources.json".into(), serde_json::to_vec_pretty(&reference)?);
                item(
                    &mut plan.report,
                    "memory",
                    "vault",
                    D::Imported,
                    "files_preserved_index_not_imported",
                );
            }
            _ => item(
                &mut plan.report,
                "memory",
                "vault",
                D::RequiresUserAction,
                "memory_root_unavailable",
            ),
        }
    }
    Ok(())
}

fn credential_reference(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(map) => map
            .iter()
            .any(|(k, v)| k.to_ascii_lowercase().contains("credential") || credential_reference(v)),
        serde_json::Value::Array(items) => items.iter().any(credential_reference),
        _ => false,
    }
}
