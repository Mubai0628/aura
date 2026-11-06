//! The rows of the shared error table. Each kind is a list of phrases; a phrase is a group of
//! fragments that must all appear in the reply's codes or words, matched in lower case.
use super::AccountSetting;

/// Fragments, in lower case, of which any one group must appear in full.
pub(super) type Phrases = &'static [&'static [&'static str]];

/// Settings of the user's own account at the Provider, in the order they are checked: OpenCode
/// Go's training switch is named next to its regions, so a data policy is told apart first.
pub(super) const ACCOUNT_SETTINGS: &[(AccountSetting, Phrases)] = &[
    (
        AccountSetting::DataPolicy,
        &[
            // OpenCode Go: Settings > Privacy > "Allow paid endpoints that train on request data".
            &["train on request data"],
            // OpenRouter, when its privacy settings leave the model no endpoint.
            &["matching your data policy"],
        ],
    ),
    (
        AccountSetting::Region,
        &[
            // OpenCode Go: "This Go model requires Global regions. Select Global in your
            // workspace's Privacy settings to use it." Either half names the setting.
            &["requires global region"],
            &["region", "privacy setting"],
        ],
    ),
    (
        AccountSetting::ClientNotAllowed,
        &[
            // Kimi For Coding, to a User-Agent outside its list (403 `access_terminated_error`).
            &["only available for coding agents"],
            // A subscription credential that serves its own vendor's client only.
            &["only authorized for use with"],
        ],
    ),
];

/// An exhausted quota, balance or plan, as opposed to a rate limit that passes.
pub(super) const QUOTA: Phrases =
    &[&["quota"], &["balance"], &["billing"], &["credit"], &["usage limit"], &["余额"], &["欠费"]];

/// A key the Provider does not accept, named under a status other than 401.
pub(super) const AUTHENTICATION: Phrases = &[
    &["invalid_api_key"],
    &["invalid api key"],
    &["incorrect api key"],
    &["api key not valid"],
    &["invalid x-api-key"],
];

/// A request or answer refused by the Provider's content policy.
pub(super) const CONTENT_POLICY: Phrases = &[
    &["content_filter"],
    &["content_policy"],
    &["content policy"],
    &["content management policy"],
    &["moderation_blocked"],
    &["safety system"],
    &["datainspectionfailed"],
];

/// A request longer than the model's context window.
pub(super) const CONTEXT_WINDOW: Phrases = &[
    &["context_length"],
    &["context length"],
    &["context window"],
    &["too many tokens"],
    &["prompt is too long"],
    &["input is too long"],
    &["reduce the length of the messages"],
];

/// A model the Provider does not serve under this name.
pub(super) const UNKNOWN_MODEL: Phrases =
    &[&["model_not_found"], &["model not found"], &["unknown model"], &["not_found_error"]];

/// A named request parameter the model does not take.
pub(super) const UNSUPPORTED_PARAMETER: Phrases =
    &[&["unsupported"], &["not supported"], &["unknown"]];

/// Every phrase list, for the table's own checks.
#[doc(hidden)]
pub fn all_phrases() -> Vec<(&'static str, Phrases)> {
    let mut all: Vec<(&'static str, Phrases)> =
        ACCOUNT_SETTINGS.iter().map(|(setting, phrases)| (setting.code(), *phrases)).collect();
    all.extend([
        ("quota", QUOTA),
        ("authentication", AUTHENTICATION),
        ("content_policy", CONTENT_POLICY),
        ("context_window", CONTEXT_WINDOW),
        ("unknown_model", UNKNOWN_MODEL),
        ("unsupported_parameter", UNSUPPORTED_PARAMETER),
    ]);
    all
}
