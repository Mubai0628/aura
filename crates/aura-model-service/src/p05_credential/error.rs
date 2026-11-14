//! Why a model credential change could not finish, each `code: 中文句` with the code its
//! settings-page answer carries; a journal's id stays at the end for the log.
use aura_domain_common::MutationJournalId;
use aura_persistence::PersistenceError;
use aura_windows_platform::CredentialStoreError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum P05CredentialJournalError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    CredentialStore(#[from] CredentialStoreError),
    #[error(
        "credential.projection_failed: 没能整理出凭据的状态，凭据操作没有完成；请刷新模型设置后重试。（{0}）"
    )]
    Projection(String),
    #[error(
        "credential.verification_mismatch: 凭据写入后读回的内容与写入时不一致，凭据操作没有完成；请重新保存密钥。"
    )]
    VerificationMismatch,
    #[error(
        "credential.reconciliation_required: 这个凭据需要先完成恢复处理才能继续使用，请到模型设置中查看。（记录 {journal_id}）"
    )]
    ReconciliationRequired { journal_id: MutationJournalId },
    #[error(
        "credential.mutation_aborted: 凭据操作在提交前被中止，没有改变任何内容，请重试。（记录 {journal_id}）"
    )]
    Aborted { journal_id: MutationJournalId },
}
