//! Why an external path grant could not be used, each `code: 中文句` with the code its answer
//! carries.
use aura_persistence::PersistenceError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExternalGrantError {
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(
        "file.invalid_external_selection: 选择的外部路径无效（必须是已存在的绝对路径），没有授权；请重新选择。"
    )]
    InvalidSelection,
    #[error(
        "file.grant_identity_unavailable: 这项外部授权里没有核实过的文件身份，不能使用；请重新授权这个路径。"
    )]
    IdentityUnavailable,
    #[error(
        "file.grant_identity_changed: 这个外部路径上的文件或文件夹已经换了（被移动、替换或重建），原来的授权不再适用；请重新授权。"
    )]
    IdentityChanged,
    #[error(
        "file.grant_suspended: 这项外部授权随所属工作区归档而暂停，没有执行；恢复工作区后会重新核实。"
    )]
    Suspended,
    #[error(
        "permission.grant_not_applicable: 这项外部授权已被撤销，没有执行；如需使用，请重新授权。"
    )]
    Revoked,
    #[error(
        "permission.grant_not_applicable: 这项外部授权已过期，没有执行；如需使用，请重新授权。"
    )]
    Expired,
    #[error(
        "file.grant_capability_unavailable: 这项外部授权不包含这次需要的权限（例如只允许读取），没有执行；如需这项操作，请重新授权。"
    )]
    CapabilityUnavailable,
    #[error(
        "file.scope_unavailable: 目标不在这项外部授权的范围内，没有执行；请授权包含它的文件夹后再试。"
    )]
    ScopeUnavailable,
    #[error(
        "permission.grant_not_applicable: 这个外部路径所依据的批准已失效或不适用于这个会话，没有执行；请重新授权这个路径。"
    )]
    PermissionGrantUnavailable,
    #[error(
        "workspace.path_resolution_failed: 读不到这个外部路径（可能已被移动、删除或没有访问权限），没有执行；请确认路径后重试。（{0}）"
    )]
    PathResolution(#[from] std::io::Error),
}
