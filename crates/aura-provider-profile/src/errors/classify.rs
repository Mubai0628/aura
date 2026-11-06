use aura_model_domain::ModelFailureCategory;

use super::rules::{
    ACCOUNT_SETTINGS, AUTHENTICATION, CONTENT_POLICY, CONTEXT_WINDOW, Phrases, QUOTA,
    UNKNOWN_MODEL, UNSUPPORTED_PARAMETER,
};

/// A Provider's error reply, as a protocol adapter reads it.
#[derive(Clone, Copy, Debug)]
pub struct ProviderErrorReply<'a> {
    pub status: u16,
    /// The reply's code-like fields (`code`, `type`, `status`), as written.
    pub codes: &'a [&'a str],
    /// The reply's own words.
    pub message: &'a str,
    /// Whether the reply names the request parameter it refuses.
    pub names_parameter: bool,
}

/// A setting of the user's own account at the Provider that refused the request. Sending the
/// request again cannot pass it: the user changes the setting there, or uses another model or
/// connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountSetting {
    /// A region the account has not allowed.
    Region,
    /// A data-use policy the account has not allowed, such as training on request data.
    DataPolicy,
    /// A plan that serves the clients on its own list only. Aura names itself and nothing else.
    ClientNotAllowed,
}

impl AccountSetting {
    /// The failure code that tells the user which setting it is.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Region => "provider_account_region",
            Self::DataPolicy => "provider_account_data_policy",
            Self::ClientNotAllowed => "provider_client_not_allowed",
        }
    }
}

/// What a reply means: its kind, and whether the same request may pass when sent again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderErrorClass {
    pub category: ModelFailureCategory,
    pub retryable: bool,
    pub account_setting: Option<AccountSetting>,
}

/// The setting of the user's account at the Provider that `codes` and `message` name, if any.
/// It holds whatever the status, and inside a stream, where there is none.
#[must_use]
pub fn account_setting(codes: &[&str], message: &str) -> Option<AccountSetting> {
    let text = lower_text(codes, message);
    ACCOUNT_SETTINGS.iter().find(|(_, phrases)| says(&text, phrases)).map(|(setting, _)| *setting)
}

/// What `reply` means. An account setting is told apart under any status; otherwise the status
/// decides before the words, so that a gateway's rewritten `server_error` under a 4xx is not
/// sent again and a 5xx is, whatever it says.
#[must_use]
pub fn classify_provider_error(reply: &ProviderErrorReply<'_>) -> ProviderErrorClass {
    use ModelFailureCategory::{
        Authentication, Authorization, ContentFilter, ContextWindowExceeded, InvalidRequest,
        QuotaOrBalance, RateLimit, Transport, UnsupportedModel, UnsupportedParameter,
    };
    if let Some(setting) = account_setting(reply.codes, reply.message) {
        return ProviderErrorClass {
            category: Authorization,
            retryable: false,
            account_setting: Some(setting),
        };
    }
    let text = lower_text(reply.codes, reply.message);
    let (category, retryable) = match reply.status {
        401 => (Authentication, false),
        402 => (QuotaOrBalance, false),
        429 if says(&text, QUOTA) => (QuotaOrBalance, false),
        429 => (RateLimit, true),
        408 => (Transport, true),
        501 | 505 => (InvalidRequest, false),
        500..=599 => (Transport, true),
        _ if says(&text, QUOTA) => (QuotaOrBalance, false),
        403 => (Authorization, false),
        _ if says(&text, AUTHENTICATION) => (Authentication, false),
        _ if says(&text, CONTENT_POLICY) => (ContentFilter, false),
        _ if says(&text, CONTEXT_WINDOW) => (ContextWindowExceeded, false),
        _ if says(&text, UNKNOWN_MODEL) => (UnsupportedModel, false),
        _ if reply.names_parameter && says(&text, UNSUPPORTED_PARAMETER) => {
            (UnsupportedParameter, false)
        }
        _ => (InvalidRequest, false),
    };
    ProviderErrorClass { category, retryable, account_setting: None }
}

fn lower_text(codes: &[&str], message: &str) -> String {
    let mut text = codes.join(" ");
    text.push(' ');
    text.push_str(message);
    text.to_lowercase()
}

fn says(text: &str, phrases: Phrases) -> bool {
    phrases.iter().any(|fragments| fragments.iter().all(|fragment| text.contains(fragment)))
}
