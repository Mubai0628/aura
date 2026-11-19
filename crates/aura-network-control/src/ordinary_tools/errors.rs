use crate::NetworkControlError as E;
use crate::connect_failure as kind;

/// Model-visible diagnostics never include reqwest's URL, headers, body or OS details.
pub fn safe_transport_code(error: &E) -> &'static str {
    match error {
        E::UserProxyFailure(failure) => failure.kind.code(),
        E::RoutedTransport { error, .. } => safe_transport_code(error),
        E::InvalidUrl | E::UrlCredentialsForbidden | E::SchemeBlocked | E::InvalidMethod => {
            "http.invalid_url_or_method"
        }
        E::ControlEndpointBlocked => "network.control_endpoint_blocked",
        E::SecretDetected { .. } => "http.model_authentication_forbidden",
        E::Timeout => "network.timeout",
        E::ResponseTooLarge | E::DownloadLimitExceeded | E::UploadLimitExceeded => {
            "network.response_too_large"
        }
        E::RedirectBlocked | E::RedirectLimitExceeded => "network.redirect_blocked",
        E::CredentialReflectionDetected { .. } | E::CredentialReflectionScanIncomplete { .. } => {
            "network.credential_reflection_blocked"
        }
        E::CredentialBindingMismatch | E::CredentialUnavailable => "http.credential_unavailable",
        E::CoreGrantBindingMismatch
        | E::CoreGrantAlreadyConsumed
        | E::GrantExpired
        | E::GrantNotApplicable => "http.authority_changed",
        E::RateLimited => "network.rate_limited",
        E::Transport(reason) => crate::connect_failure::code(reason),
        _ => "network.request_refused",
    }
}

/// Core's line for a failure [`safe_transport_code`] names: the code, then what happened, whether
/// the request reached the target when the kind of failure tells it, and what to do next. Like the
/// code, it never carries the library's or the system's own words, the URL or a header.
pub fn safe_transport_line(error: &E) -> String {
    if let E::UserProxyFailure(failure) = error {
        return failure.to_string();
    }
    format!("{}: {}", safe_transport_code(error), sentence(error))
}

fn sentence(error: &E) -> &'static str {
    match error {
        E::RoutedTransport { error, request_not_started: false, .. }
            if matches!(error.as_ref(), E::UrlCredentialsForbidden) =>
        {
            "已收到目标的 HTTP 跳转回应。跳转地址带有用户名或密码，Aura 停在这一跳。请使用无凭据的网址。"
        }
        E::RoutedTransport { error, request_not_started: true, .. } if uncertain(error) => {
            "连接目标时没有成功，尚未向目标发出 HTTP 请求。请确认网络和目标服务正常后再重试。"
        }
        E::RoutedTransport { error, .. } => sentence(error),
        E::UserProxyFailure(_) => {
            "本机代理没有接通，尚未向目标发出 HTTP 请求。请确认代理软件已运行，或在 设置 → 模型 → 网络代理 里换一个代理。"
        }
        E::InvalidUrl => {
            "地址不是有效的 http:// 或 https:// 地址，尚未向目标发出 HTTP 请求。请检查地址后再调用。"
        }
        E::UrlCredentialsForbidden => {
            "地址里带了用户名或密码，这样的地址不会发出，尚未向目标发出 HTTP 请求。需要认证时请用工具设置里绑定的凭据。"
        }
        E::SchemeBlocked => "只支持 http:// 和 https:// 地址，尚未向目标发出 HTTP 请求。",
        E::InvalidMethod => {
            "请求方法无效，尚未向目标发出 HTTP 请求。请用 GET、POST、PUT、PATCH、DELETE 这类标准方法。"
        }
        E::ControlEndpointBlocked => {
            "目标是 Aura 正在使用的控制端点，不能向该地址发送工具请求，尚未向目标发出 HTTP 请求。请改用项目 API 地址。"
        }
        E::SecretDetected { .. } => {
            "认证信息只能通过工具凭据提供（工具设置里配置凭据与允许网站），尚未向目标发出 HTTP 请求。"
        }
        E::Timeout => {
            "请求没有在时限内完成，不能确认目标请求是否已经发出。可以稍后重试；写操作请先确认它有没有生效。"
        }
        E::UploadLimitExceeded => {
            "要发送的内容超过了这次调用允许的大小，尚未向目标发出 HTTP 请求。请减小请求内容后再调用。"
        }
        E::ResponseTooLarge | E::DownloadLimitExceeded => {
            "回应超过了这次调用允许的大小，已停止接收，内容没有保存（请求已经发出）。可以请求更小的内容或范围。"
        }
        E::RedirectBlocked => {
            "目标要求跳转到另一个地址，这次调用不允许跟随这个跳转，跳转没有跟随。需要时可以直接请求跳转后的地址。"
        }
        E::RedirectLimitExceeded => {
            "跳转次数超过了上限，后面的跳转没有跟随。请直接请求最终的地址。"
        }
        E::CredentialReflectionDetected { .. } => {
            "目标的回应里出现了这次请求所带的凭据，为保护凭据，回应没有交给模型（请求已经发出）。请换一个不会原样返回凭据的接口。"
        }
        E::CredentialReflectionScanIncomplete { .. } => {
            "没能确认目标的回应里不含这次请求所带的凭据，为保护凭据，回应没有交给模型（请求已经发出）。可以稍后重试。"
        }
        E::CredentialBindingMismatch | E::CredentialUnavailable => {
            "这次请求要用的凭据现在取不到，或与绑定的不一致。请用户在工具设置里检查这个凭据的绑定后再调用。"
        }
        E::CoreGrantBindingMismatch
        | E::CoreGrantAlreadyConsumed
        | E::GrantExpired
        | E::GrantNotApplicable => {
            "这次调用的授权已经失效或与请求对不上（例如等待期间权限、设置或批准变了），请求被 Aura 停下。可以重新调用；写操作请先确认它有没有生效。"
        }
        E::RateLimited => {
            "对这个网站的请求太多或太快，超过了 Aura 自己的限额（同时进行的请求数或一段时间里的请求量），这次请求被停下。请稍等片刻再调用，或减少同时发出的请求。"
        }
        E::Transport(reason) => match reason.as_str() {
            kind::REFUSED => {
                "目标没有程序在监听（连接被拒绝），尚未向目标发出 HTTP 请求。如果它是本机的服务，先启动它再重试。"
            }
            kind::HOST_NOT_FOUND => {
                "找不到目标主机（域名解析失败），尚未向目标发出 HTTP 请求。请检查地址是否写对，确认网络可用后再重试。"
            }
            kind::TLS_FAILED => {
                "与目标的 TLS 握手失败（证书或协议不被接受），尚未向目标发出 HTTP 请求。如果它只提供 HTTP，请改用 http://。"
            }
            kind::RESET => {
                "与目标的连接被对方断开，不能确认目标请求是否已经发出。可以稍后重试；写操作请先确认它有没有生效。"
            }
            kind::CONNECT_FAILED => {
                "无法连接目标，尚未向目标发出 HTTP 请求。请确认目标服务已启动、地址和端口正确后再重试。"
            }
            _ => {
                "请求在传输中途失败，不能确认目标请求是否已经发出。可以稍后重试；写操作请先确认它有没有生效。"
            }
        },
        E::DestinationBlocked => {
            "目标地址被 Aura 的网络规则拦下（例如云服务的元数据地址，或解析结果与批准时不一致），尚未向目标发出 HTTP 请求。请换一个地址。"
        }
        E::PrivateNetworkApprovalRequired => {
            "目标是本机或局域网里的地址，这次调用没有访问这类地址的许可，尚未向目标发出 HTTP 请求。"
        }
        E::DataEgressApprovalRequired => {
            "这次请求会把工作区或用户的数据发出去，没有得到许可，尚未向目标发出 HTTP 请求。"
        }
        E::CredentialMaterialForbidden => {
            "请求里带有凭据类的数据，Aura 不会这样发送，尚未向目标发出 HTTP 请求。需要认证时请用工具设置里绑定的凭据。"
        }
        E::ContentEncodingUnsupported => {
            "目标的回应用了 Aura 不支持的压缩方式，内容没有读出（请求已经发出）。"
        }
        E::DecompressionFailed => {
            "目标的回应解压失败，内容没有读出（请求已经发出）。可以稍后重试。"
        }
        E::TlsServerNameMismatch => {
            "目标的证书与地址里的主机名不符，连接没有建立，尚未向目标发出 HTTP 请求。请检查地址是否写对。"
        }
        E::ProxyUnauthorized | E::ProxyProtocol | E::ProxyInterrupted => {
            "经过代理转发时出错，不能确认目标请求是否已经发出。可以稍后重试；反复出现时请检查网络代理设置。"
        }
        _ => {
            "Aura 的网络网关没有完成这次请求，不能确认目标请求是否已经发出。可以稍后重试；写操作请先确认它有没有生效。"
        }
    }
}

/// A failure that, on its own, leaves open whether the request reached the target.
fn uncertain(error: &E) -> bool {
    match error {
        E::Timeout => true,
        E::Transport(reason) => !matches!(
            reason.as_str(),
            kind::REFUSED | kind::HOST_NOT_FOUND | kind::TLS_FAILED | kind::CONNECT_FAILED
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_supplied_authentication_has_a_dedicated_transport_code() {
        let error = E::SecretDetected { location: "request.body".into(), kind: "api_token".into() };
        assert_eq!(safe_transport_code(&error), "http.model_authentication_forbidden");
    }

    /// The line names its code first, says whether the request left when the failure tells, and
    /// never repeats the transport's own words.
    #[test]
    fn a_transport_line_is_the_code_and_core_s_own_sentence() {
        let refused = safe_transport_line(&E::Transport(kind::REFUSED.into()));
        assert!(refused.starts_with("network.connection_refused: "), "{refused}");
        assert!(refused.contains("尚未向目标发出 HTTP 请求"), "{refused}");
        let raw = safe_transport_line(&E::Transport("response body read failed".into()));
        assert!(raw.starts_with("network.transport_failed: "), "{raw}");
        assert!(!raw.contains("body read") && raw.contains("不能确认目标请求是否已经发出"));
        let unsent = safe_transport_line(&E::RoutedTransport {
            error: Box::new(E::Timeout),
            routing: vec![],
            request_not_started: true,
        });
        assert!(unsent.starts_with("network.timeout: ") && unsent.contains("尚未向目标发出"));
        for error in [
            E::RedirectBlocked,
            E::RateLimited,
            E::ResponseTooLarge,
            E::DestinationBlocked,
            E::GrantExpired,
            E::CredentialUnavailable,
            E::QuarantineNotFound,
        ] {
            let line = safe_transport_line(&error);
            assert!(line.starts_with(&format!("{}: ", safe_transport_code(&error))), "{line}");
            assert!(line.len() <= 512 && !line.contains('\n'), "{line}");
        }
    }
}
