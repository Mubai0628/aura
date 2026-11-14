//! Why fetching the model list could not run, each `code: 中文句` with the code its settings-page
//! answer carries.
use aura_persistence::PersistenceError;
use thiserror::Error;

use super::super::{
    config_journal::ModelConfigJournalError, configuration_transport::ConfigurationTransportError,
};

#[derive(Debug, Error)]
pub(crate) enum ModelDiscoveryServiceError {
    #[error(
        "configuration.endpoint.invalid: 保存的连接配置不能用来获取模型列表，请检查 Base URL 与模型目录路径。"
    )]
    InvalidConfiguration,
    #[error(
        "models.configuration.revision_conflict: 模型设置已在别处更新，请刷新后再获取模型列表。"
    )]
    ConfigurationRevisionChanged,
    #[error(
        "credential.unavailable: 这个连接的凭据暂不可用，无法获取模型列表；请重新保存 API Key 后再试。"
    )]
    CredentialUnavailable,
    #[error(
        "models.configuration.mutation_in_progress: 另一项模型设置的修改正在进行，请等它完成后再试。"
    )]
    ModelConfigMutationInProgress,
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Transport(#[from] ConfigurationTransportError),
    #[error(transparent)]
    ConfigJournal(#[from] ModelConfigJournalError),
    #[error("persistence.revision_overflow: 获取模型列表的状态没能更新，请刷新后重试。")]
    RevisionOverflow,
    #[error("models.projection.unavailable: 暂时读不到获取模型列表的状态，请刷新后再看。")]
    MissingProjection,
    #[error("operation.not_found: 这次获取模型列表的操作已不存在，请刷新。")]
    OperationNotFound,
    #[error("internal.unexpected: 获取模型列表意外中断，请重试。")]
    WorkerTerminated,
    #[error(
        "operation.revision_conflict: 获取模型列表的状态已经变化（应为第 {expected} 版，实为第 {actual} 版），请刷新后再试。"
    )]
    OperationRevisionConflict { expected: u64, actual: u64 },
}
