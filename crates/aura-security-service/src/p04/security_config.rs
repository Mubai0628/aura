use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct P04SecurityConfigValidator;

impl ModuleConfigValidator for P04SecurityConfigValidator {
    fn validate(
        &self,
        module: ConfigModule,
        document: &ModuleConfigDocument,
    ) -> Result<(), ConfigValidationError> {
        if module != ConfigModule::Security {
            return Ok(());
        }
        SecurityConfig::from_document(document)
            .map(|_| ())
            .map_err(|error| ConfigValidationError::Semantic(error.to_string()))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct SecurityConfig {
    pub(super) native_sandbox: NativeSandboxConfig,
    pub(super) external_extensions: ExternalExtensionsConfig,
    pub(super) approval: ApprovalConfig,
    pub(super) network: NetworkConfig,
    pub(super) host_preparation: HostPreparationConfig,
}

impl SecurityConfig {
    pub(super) fn from_document(document: &ModuleConfigDocument) -> Result<Self> {
        let object = document.values.clone().into_iter().collect();
        let config: Self =
            serde_json::from_value(Value::Object(object)).context("config.security_invalid: 安全设置（security.toml）里有不认识的项或值的格式不对，没有采用；请改正后再保存，或到设置里查看恢复选项。")?;
        config.validate()?;
        Ok(config)
    }

    pub(super) fn validate(&self) -> Result<()> {
        ensure!(
            self.native_sandbox.default_mode == "compatibility_appcontainer",
            "config.security_invalid: 安全设置里 [native_sandbox] 的 default_mode 只能是 compatibility_appcontainer，没有采用；请改正后再保存，或到设置里查看恢复选项。"
        );
        ensure!(
            matches!(
                self.external_extensions.default_isolation.as_str(),
                "compatibility"
                    | "strict_appcontainer"
                    | "native_managed_user"
                    | "managed_user_networked"
            ),
            "config.security_invalid: 安全设置里 [external_extensions] 的 default_isolation 只能是 compatibility、strict_appcontainer、native_managed_user 或 managed_user_networked，没有采用；请改正后再保存，或到设置里查看恢复选项。"
        );
        let _ = (
            self.native_sandbox.allow_observed_native,
            self.native_sandbox.allow_unrestricted_inline_scripts_in_full_access,
            self.approval.show_detailed_by_default,
            self.network.private_network_requires_grant,
            self.network.quarantine_executables,
            self.host_preparation.auto_probe,
            self.host_preparation.maintain_nul_permission_on_login,
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct NativeSandboxConfig {
    pub(super) default_mode: String,
    pub(super) allow_observed_native: bool,
    pub(super) allow_unrestricted_inline_scripts_in_full_access: bool,
}

impl Default for NativeSandboxConfig {
    fn default() -> Self {
        Self {
            default_mode: "compatibility_appcontainer".to_owned(),
            allow_observed_native: true,
            allow_unrestricted_inline_scripts_in_full_access: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ExternalExtensionsConfig {
    pub(super) default_isolation: String,
}

impl Default for ExternalExtensionsConfig {
    fn default() -> Self {
        Self { default_isolation: "compatibility".to_owned() }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ApprovalConfig {
    pub(super) show_detailed_by_default: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct NetworkConfig {
    pub(super) private_network_requires_grant: bool,
    pub(super) quarantine_executables: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self { private_network_requires_grant: true, quarantine_executables: true }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct HostPreparationConfig {
    pub(super) auto_probe: bool,
    pub(super) maintain_nul_permission_on_login: bool,
}

impl Default for HostPreparationConfig {
    fn default() -> Self {
        Self { auto_probe: true, maintain_nul_permission_on_login: false }
    }
}

pub(super) fn parse_default_permission_mode(
    document: &ModuleConfigDocument,
) -> Result<PermissionMode> {
    match document.values.get("default_permission_mode") {
        None => Ok(PermissionMode::ReadOnly),
        Some(Value::String(value)) if value == "read_only" => Ok(PermissionMode::ReadOnly),
        Some(Value::String(value)) if value == "workspace_read_write" => {
            Ok(PermissionMode::WorkspaceReadWrite)
        }
        Some(Value::String(value)) if value == "full_access" => Ok(PermissionMode::FullAccess),
        Some(_) => Err(anyhow!(
            "config.aura_invalid: aura.toml 里的 default_permission_mode 只能是 read_only、workspace_read_write 或 full_access，没有采用；请改正后再保存，或到设置里查看恢复选项。"
        )),
    }
}
