use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolCoreRetrySourceV1 {
    Http408 { retry_after: Option<GatewayRetryAfterV1> },
    Http425 { retry_after: Option<GatewayRetryAfterV1> },
    Http429 { retry_after: Option<GatewayRetryAfterV1> },
    Http502 { retry_after: Option<GatewayRetryAfterV1> },
    Http503 { retry_after: Option<GatewayRetryAfterV1> },
    Http504 { retry_after: Option<GatewayRetryAfterV1> },
    GatewayRateLimited,
    GatewayTimeout,
    GatewayProxyInterrupted,
}

impl ToolCoreRetrySourceV1 {
    #[must_use]
    pub(crate) const fn retry_after_ms(self) -> Option<u64> {
        match self {
            Self::Http408 { retry_after }
            | Self::Http425 { retry_after }
            | Self::Http429 { retry_after }
            | Self::Http502 { retry_after }
            | Self::Http503 { retry_after }
            | Self::Http504 { retry_after } => match retry_after {
                Some(value) => Some(value.delay_ms()),
                None => None,
            },
            Self::GatewayRateLimited | Self::GatewayTimeout | Self::GatewayProxyInterrupted => None,
        }
    }

    #[must_use]
    pub(crate) const fn safe_code(self) -> &'static str {
        match self {
            Self::Http408 { .. } => "http_408",
            Self::Http425 { .. } => "http_425",
            Self::Http429 { .. } => "http_429",
            Self::Http502 { .. } => "http_502",
            Self::Http503 { .. } => "http_503",
            Self::Http504 { .. } => "http_504",
            Self::GatewayRateLimited => "gateway_rate_limited",
            Self::GatewayTimeout => "gateway_timeout",
            Self::GatewayProxyInterrupted => "gateway_proxy_interrupted",
        }
    }
}
