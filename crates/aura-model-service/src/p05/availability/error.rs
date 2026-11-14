//! Why an availability test could not run, each `code: 中文句` with the code its settings-page
//! answer carries.
use aura_model_domain::ModelAvailabilityPrerequisiteFailure;
use aura_model_transport::ModelAdapterError;
use aura_persistence::PersistenceError;
use thiserror::Error;

use super::super::{
    config_journal::ModelConfigJournalError, configuration_transport::ConfigurationTransportError,
};

#[derive(Debug, Error)]
pub(crate) enum ModelAvailabilityServiceError {
    #[error(
        "configuration.availability_test_prerequisite_missing: 保存的模型配置还不完整，暂时不能测试可用性；请先补全连接与凭据。"
    )]
    Prerequisite(ModelAvailabilityPrerequisiteFailure),
    #[error("persistence.operation_failed: 可用性测试没能记录下来，测试没有开始；请重试。")]
    Persistence(#[from] PersistenceError),
    #[error(
        "transport.operation_failed: 可用性测试的网络请求没能开始；请检查网络与代理设置后重试。"
    )]
    Transport(#[from] ConfigurationTransportError),
    #[error("provider.request_compilation_failed: 兼容性重试的请求没能生成，测试已停止；请重试。")]
    Adapter(#[from] ModelAdapterError),
    #[error(
        "model.availability_learning_failed: 测出的兼容设置没能保存，测试已停止；请刷新模型设置后重试。"
    )]
    ConfigJournal(#[from] ModelConfigJournalError),
    #[error(
        "compatibility.output_token_field_conflict: 测出的输出 Token 字段与当前的兼容设置冲突，没有采用；请刷新后再测。"
    )]
    OutputTokenFieldAliasConflict,
    #[error("persistence.revision_overflow: 可用性测试的状态没能更新，请刷新后重试。")]
    RevisionOverflow,
    #[error("models.projection.unavailable: 暂时读不到可用性测试的状态，请刷新后再看。")]
    MissingProjection,
    #[error("operation.not_found: 这次可用性测试已不存在，请刷新。")]
    OperationNotFound,
    #[error("internal.unexpected: 可用性测试意外中断，请重试。")]
    WorkerTerminated,
    #[error(
        "operation.revision_conflict: 可用性测试的状态已经变化（应为第 {expected} 版，实为第 {actual} 版），请刷新后再试。"
    )]
    OperationRevisionConflict { expected: u64, actual: u64 },
}
