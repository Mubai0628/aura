//! Why one HTTP exchange produced no answer, and what the user reads about it. A request Aura's
//! own network rules refused never left this computer; anything that failed after it may have
//! reached the server is unknown.
use crate::mcp::connection::RequestFailure;
use aura_network_control::NetworkControlError;

pub(super) enum Failure {
    NotSent(String),
    Lost(String),
    /// The server no longer knows the session: it did not process the request.
    Expired,
    /// The server refused the credential (401): it did not process the request.
    Unauthorized,
    TimedOut,
    Cancelled,
}

impl From<Failure> for RequestFailure {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::NotSent(reason) => Self::NotSent(reason),
            Failure::Lost(reason) => Self::Lost(reason),
            Failure::Expired => Self::NotSent("服务器的会话已过期".into()),
            Failure::Unauthorized => {
                Self::NotSent("服务器要求登录（HTTP 401），请在设置里检查密钥或登录".into())
            }
            Failure::TimedOut => Self::TimedOut,
            Failure::Cancelled => Self::Cancelled,
        }
    }
}

pub(super) fn classify(error: NetworkControlError) -> Failure {
    use NetworkControlError as E;
    let reason = describe(&error);
    match error {
        E::RoutedTransport { request_not_started: true, .. }
        | E::InvalidUrl
        | E::UrlCredentialsForbidden
        | E::SchemeBlocked
        | E::InvalidMethod
        | E::DestinationBlocked
        | E::ControlEndpointBlocked
        | E::PrivateNetworkApprovalRequired
        | E::DataEgressApprovalRequired
        | E::SecretDetected { .. }
        | E::CredentialMaterialForbidden
        | E::GrantExpired
        | E::GrantNotApplicable
        | E::CoreGrantAlreadyConsumed
        | E::CoreGrantBindingMismatch
        | E::RateLimited
        | E::UploadLimitExceeded
        | E::UserProxyFailure(_)
        | E::ProxyUnauthorized
        | E::ProxyProtocol
        | E::TlsServerNameMismatch
        | E::CredentialBindingMismatch
        | E::CredentialUnavailable => Failure::NotSent(reason),
        _ => Failure::Lost(reason),
    }
}

pub(crate) fn describe(error: &NetworkControlError) -> String {
    use NetworkControlError as E;
    match error {
        E::RoutedTransport { error, .. } => describe(error),
        E::RedirectBlocked | E::RedirectLimitExceeded => {
            "服务器要求跳转到别的地址，Aura 不跟随跳转".into()
        }
        // A grant binds the address's own class: a domain name that resolves to this computer
        // or the local network no longer matches it.
        E::DestinationBlocked
        | E::PrivateNetworkApprovalRequired
        | E::ControlEndpointBlocked
        | E::CoreGrantBindingMismatch => {
            "这个地址连到了不允许的位置（域名只能连到公网；本机或局域网的服务器请直接写 IP 地址或 localhost）"
                .into()
        }
        E::SecretDetected { .. } | E::CredentialMaterialForbidden => {
            "请求里有像密钥或口令的内容，Aura 没有发出".into()
        }
        E::CredentialReflectionDetected { .. } | E::CredentialReflectionScanIncomplete { .. } => {
            "服务器的回应里带着这个服务器的登录凭据，Aura 丢弃了这次回应".into()
        }
        E::CredentialUnavailable | E::CredentialBindingMismatch => {
            "登录凭据不可用，请在设置里重新设置".into()
        }
        E::Timeout => "连接超时".into(),
        E::Transport(detail) => format!("连接失败：{detail}"),
        E::DownloadLimitExceeded | E::ResponseTooLarge => "服务器的回应太大".into(),
        other => format!("网络请求没有完成：{other}"),
    }
}
