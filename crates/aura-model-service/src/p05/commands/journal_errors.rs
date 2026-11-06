//! What the user is told when a model-config command fails.
use std::collections::BTreeMap;

use aura_config::SettingsFacadeError;
use aura_domain_common::{CorrelationId, ErrorCategory, ErrorCode};
use aura_model_config::{ModelConfigError, ModelConfigValidationError};
use aura_persistence::PersistenceError;
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use serde_json::Value;

use super::super::config_journal::ModelConfigJournalError;

pub(in crate::p05) fn journal_error(
    error: ModelConfigJournalError,
    correlation_id: CorrelationId,
) -> IpcErrorEnvelope {
    #[cfg(any(test, feature = "test-hooks"))]
    eprintln!("P05 model config command failed: {error:?}");
    let (code, category, message, retryable, details) = match error {
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::MissingChatOutputTokenField { .. },
        )) => (
            "models.endpoint.chat_output_token_field_required",
            ErrorCategory::Validation,
            "OpenAI Chat Completions 连接必须选择 Output Token Field。",
            false,
            endpoint_field_details("chat_output_token_field"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::MissingChatInstructionRole { .. },
        )) => (
            "models.endpoint.chat_instruction_role_required",
            ErrorCategory::Validation,
            "OpenAI Chat Completions 连接必须选择 Chat Instruction Role。",
            false,
            endpoint_field_details("chat_instruction_role"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidRoutingField { field, .. },
        )) => (
            "models.endpoint.field_not_allowed",
            ErrorCategory::Validation,
            "该字段不适用于所选的 API 类型，请核对后重新保存。",
            false,
            endpoint_field_details(field),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidAuthScheme { .. },
        )) => (
            "models.endpoint.invalid_auth_scheme",
            ErrorCategory::Validation,
            "当前认证方式不适用于所选的 API 类型。",
            false,
            endpoint_field_details("auth_scheme"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidBaseUrl { .. },
        )) => (
            "models.endpoint.invalid_base_url",
            ErrorCategory::Validation,
            "Base URL 无效，请填写不含用户信息、查询参数或片段的有效地址。",
            false,
            endpoint_field_details("base_url"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidModelsPath { .. },
        )) => (
            "models.endpoint.invalid_models_path",
            ErrorCategory::Validation,
            "模型目录请求路径无效，请填写同一地址下的 HTTP 路径。",
            false,
            endpoint_field_details("models_path"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidTimeout { field, .. },
        )) => (
            "models.endpoint.invalid_timeout",
            ErrorCategory::Validation,
            "超时必须是大于 0 的整数。",
            false,
            endpoint_field_details(field),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::InvalidParallelLimit { .. },
        )) => (
            "models.endpoint.invalid_parallel_limit",
            ErrorCategory::Validation,
            "并发请求上限必须是大于 0 的整数。",
            false,
            endpoint_field_details("max_parallel_model_requests"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::BlankValue { path },
        )) if path == "endpoint.display_name" => (
            "models.endpoint.invalid_display_name",
            ErrorCategory::Validation,
            "请输入连接名称。",
            false,
            endpoint_field_details("display_name"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::BlankValue { path },
        )) if path == "endpoint.base_url" => (
            "models.endpoint.invalid_base_url",
            ErrorCategory::Validation,
            "Base URL 无效，请填写不含用户信息、查询参数或片段的有效地址。",
            false,
            endpoint_field_details("base_url"),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Validation(
            ModelConfigValidationError::BlankValue { path },
        )) if path.starts_with("endpoint.") => (
            "models.endpoint.invalid_field",
            ErrorCategory::Validation,
            "有一个连接字段只有空白，请填写内容或清空后再保存。",
            false,
            endpoint_field_details(path.trim_start_matches("endpoint.")),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::ProviderNotFoundById(_)) => (
            "models.provider.not_found",
            ErrorCategory::NotFound,
            "这个服务商已不存在，请刷新模型设置。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::DuplicateProviderModelName {
            ..
        }) => (
            "models.profile.already_exists",
            ErrorCategory::Conflict,
            "这个连接下已经添加过同一个模型。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::Settings(
            aura_config::SettingsFacadeError::RevisionConflict { .. },
        ))
        | ModelConfigJournalError::ModelConfig(
            ModelConfigError::EndpointObjectRevisionConflict { .. }
            | ModelConfigError::ProfileObjectRevisionConflict { .. },
        )
        | ModelConfigJournalError::Persistence(
            aura_persistence::PersistenceError::SessionModelBindingRevisionConflict,
        ) => (
            "models.configuration.revision_conflict",
            ErrorCategory::Conflict,
            "模型设置已在别处更新，请刷新后再试。",
            true,
            BTreeMap::new(),
        ),
        // Another model-config change still holds the domain or the journal lease; it ends on
        // its own, so trying again succeeds.
        ModelConfigJournalError::Persistence(PersistenceError::MutationLeaseConflict)
        | ModelConfigJournalError::Settings(
            SettingsFacadeError::DomainBusy { .. } | SettingsFacadeError::BundlePending { .. },
        )
        | ModelConfigJournalError::ModelConfig(ModelConfigError::Settings(
            SettingsFacadeError::DomainBusy { .. } | SettingsFacadeError::BundlePending { .. },
        )) => (
            "models.configuration.busy",
            ErrorCategory::Conflict,
            "另一项模型设置修改还没完成，这次修改没有保存；请稍后重试。",
            true,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::Aborted(_) => (
            "models.configuration.aborted",
            ErrorCategory::Internal,
            "这次修改在生效前被中止，模型设置没有改变，请重试。",
            false,
            BTreeMap::new(),
        ),
        // Binding refusals name their real reason; the catch-all below would leave
        // the user with an unexplained "could not be committed" and no next step.
        ModelConfigJournalError::ModelConfig(
            ModelConfigError::InvalidExecutionCredentialBinding,
        ) => (
            "models.binding.credential_required",
            ErrorCategory::Permission,
            "这个模型所在的连接还没有可用的凭据，暂时不能选用；请在模型设置里保存 API Key 后再试。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(
            ModelConfigError::OutputLimitConfigurationRequired(_),
        ) => (
            "models.binding.output_limit_required",
            ErrorCategory::Validation,
            "这个模型需要先设置最大输出 Token 上限，才能选用。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::ProfileDisabled(_)) => (
            "models.binding.profile_disabled",
            ErrorCategory::Conflict,
            "这个模型已停用，请在模型设置中启用或选择其他模型。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::ProfileNotFound(_)) => (
            "models.binding.profile_missing",
            ErrorCategory::NotFound,
            "这个模型已不存在，请在模型设置中重新选择。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(
            ModelConfigError::EndpointNotFound(_) | ModelConfigError::ProviderNotFound(_),
        ) => (
            "models.binding.endpoint_missing",
            ErrorCategory::NotFound,
            "这个模型所属的连接已不存在，请在模型设置中重新配置。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::CatalogModelNotChat { .. }) => (
            "models.catalog.model_not_chat",
            ErrorCategory::Validation,
            "这个目录条目不是对话模型，不能添加。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::CatalogEntryNotForModel {
            ..
        }) => (
            "models.catalog.entry_not_for_model",
            ErrorCategory::Validation,
            "这个目录条目和模型名称对不上，不能选用。",
            false,
            BTreeMap::new(),
        ),
        // The catalog was refreshed after the settings page listed the entry.
        ModelConfigJournalError::ModelConfig(ModelConfigError::CatalogObjectHashMismatch) => (
            "models.catalog.entry_changed",
            ErrorCategory::Conflict,
            "模型目录刚更新过，这一条已经变了，这次没有保存；请重新打开设置再选。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::CatalogModelOnAnotherRoute {
            route_id,
            ..
        }) => (
            "models.catalog.model_on_another_route",
            ErrorCategory::Validation,
            "这个模型要走这家服务商的另一条连接，请添加到那条连接下。",
            false,
            BTreeMap::from([("route".to_owned(), Value::String(route_id))]),
        ),
        ModelConfigJournalError::ModelConfig(ModelConfigError::CatalogModelRouteUnsupported {
            ..
        }) => (
            "models.catalog.model_route_unsupported",
            ErrorCategory::Validation,
            "这个模型走的协议 Aura 还不支持，不能添加到这家服务商的连接。",
            false,
            BTreeMap::new(),
        ),
        ModelConfigJournalError::ReconciliationRequired(_) => (
            "persistence.reconciliation_required",
            ErrorCategory::Unavailable,
            "模型设置需要先完成恢复才能修改，请到设置中查看恢复选项。",
            false,
            BTreeMap::new(),
        ),
        // The message sends the user to the local log, so the log must carry the cause.
        error => {
            aura_core_base::append_core_log(
                "model_config_command_failed",
                serde_json::json!({
                    "correlation_id": correlation_id,
                    "reason": error.to_string(),
                }),
            );
            (
                "models.configuration.failed",
                ErrorCategory::Internal,
                "模型设置没有保存成功，配置保持不变；请重试，仍不行请查看本机日志。",
                false,
                BTreeMap::new(),
            )
        }
    };
    IpcErrorEnvelope {
        code: ErrorCode::new(code).expect("static ErrorCode is valid"),
        category,
        message: message.to_owned(),
        retryable,
        correlation_id,
        terminal: IpcRequestTerminal::Failed,
        details,
    }
}

fn endpoint_field_details(field: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([("field".to_owned(), Value::String(field.to_owned()))])
}

#[cfg(test)]
mod tests {
    use aura_config::{ConfigModule, SettingsFacadeError};
    use aura_core_base::core_log::CoreLogRoute;
    use aura_domain_common::CorrelationId;
    use aura_model_config::ModelConfigError;
    use aura_persistence::PersistenceError;

    use super::super::super::config_journal::ModelConfigJournalError;
    use super::journal_error;

    #[test]
    fn another_change_still_finishing_is_reported_busy_and_retryable() {
        let domain_busy = || SettingsFacadeError::DomainBusy {
            message: "configuration domain owned by another operation: models".to_owned(),
        };
        let bundle_pending =
            || SettingsFacadeError::BundlePending { transaction_id: "jnl_other".to_owned() };
        for error in [
            ModelConfigJournalError::Persistence(PersistenceError::MutationLeaseConflict),
            ModelConfigJournalError::Settings(domain_busy()),
            ModelConfigJournalError::Settings(bundle_pending()),
            ModelConfigJournalError::ModelConfig(ModelConfigError::Settings(domain_busy())),
            ModelConfigJournalError::ModelConfig(ModelConfigError::Settings(bundle_pending())),
        ] {
            let described = format!("{error:?}");
            let envelope = journal_error(error, CorrelationId::new());
            assert_eq!(envelope.code.as_str(), "models.configuration.busy", "{described}");
            assert!(envelope.retryable, "{described}");
        }
        let unavailable = journal_error(
            ModelConfigJournalError::Settings(SettingsFacadeError::DomainUnavailable {
                module: ConfigModule::Models,
                message: "an unresolved configuration bundle includes this domain".to_owned(),
            }),
            CorrelationId::new(),
        );
        assert_eq!(unavailable.code.as_str(), "models.configuration.failed");
    }

    #[test]
    fn an_unexplained_failure_leaves_its_cause_in_the_core_log() {
        let directory = tempfile::tempdir().expect("log directory");
        let path = directory.path().join("core.jsonl");
        let correlation_id = CorrelationId::new();
        {
            let _route = CoreLogRoute::to(path.clone());
            let envelope = journal_error(
                ModelConfigJournalError::Persistence(PersistenceError::InvalidRequest(
                    "the cause the user is sent to find".to_owned(),
                )),
                correlation_id,
            );
            assert_eq!(envelope.code.as_str(), "models.configuration.failed");
            assert!(envelope.message.contains("本机日志"), "{}", envelope.message);
        }
        let log = std::fs::read_to_string(&path).expect("Core log");
        let entry = log
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("log entry"))
            .find(|entry| {
                entry["event"] == "model_config_command_failed"
                    && entry["fields"]["correlation_id"] == correlation_id.to_string()
            })
            .expect("the failure is logged with its correlation id");
        assert!(
            entry["fields"]["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("the cause the user is sent to find")),
            "{entry}"
        );
    }
}
