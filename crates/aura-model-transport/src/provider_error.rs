//! A Provider's error reply, read the same way for every protocol adapter and told apart by
//! the shared table in `aura_provider_profile`.
use aura_model_domain::{ModelFailureCategory, ModelRetryClass, NormalizedModelFailure};
use aura_provider_profile::{
    AccountSetting, ProviderErrorReply, account_setting, classify_provider_error,
};
use serde_json::Value;

use crate::{ModelAdapterError, ProviderErrorResponse, retry_after_milliseconds};

/// Where a protocol's error reply names its code, and what a reply naming no code or message
/// is recorded with.
#[derive(Clone, Copy, Debug)]
pub struct ProviderErrorFields {
    /// The fields of the error object naming its code, in the order the protocol prefers them.
    pub code: &'static [&'static str],
    pub fallback_code: &'static str,
    pub fallback_message: &'static str,
}

/// The code-like fields every reply is matched by, whichever the protocol records.
const MATCHED_CODE_FIELDS: [&str; 3] = ["code", "type", "status"];

/// No protocol's own fields: a reply read by the fields every reply is matched by.
const ANY_PROTOCOL: ProviderErrorFields =
    ProviderErrorFields { code: &[], fallback_code: "", fallback_message: "" };

/// The setting of the user's account at the Provider that a failed reply's `body` names, read
/// as a protocol adapter reads it, whichever the protocol.
#[must_use]
pub fn provider_reply_account_setting(body: &[u8]) -> Option<AccountSetting> {
    let body = serde_json::from_slice::<Value>(body).ok()?;
    let reply = ErrorReply::read(&body, ANY_PROTOCOL);
    account_setting(&reply.codes, reply.message.unwrap_or_default())
}

/// A failed HTTP reply, as a failure of its kind. A reply naming a setting of the user's
/// account at the Provider is recorded under that setting's code, with the Provider's words.
#[must_use]
pub fn provider_http_failure(
    response: &ProviderErrorResponse,
    fields: ProviderErrorFields,
) -> NormalizedModelFailure {
    let body = serde_json::from_slice::<Value>(&response.body).ok();
    let reply =
        body.as_ref().map_or_else(ErrorReply::default, |body| ErrorReply::read(body, fields));
    let message = reply.message.unwrap_or(fields.fallback_message);
    let class = classify_provider_error(&ProviderErrorReply {
        status: response.status,
        codes: &reply.codes,
        message,
        names_parameter: reply.names_parameter,
    });
    let code = class
        .account_setting
        .map_or_else(|| reply.code.unwrap_or(fields.fallback_code), |setting| setting.code());
    NormalizedModelFailure {
        category: class.category,
        code: code.to_owned(),
        message: message.to_owned(),
        retry_class: if class.retryable {
            ModelRetryClass::TransparentRetry {
                retry_after_ms: retry_after_milliseconds(&response.headers),
            }
        } else {
            ModelRetryClass::NotRetryable
        },
        provider_status: Some(response.status),
        provider_request_id: response.provider_request_id.clone(),
        provider_error_ref: response.raw_error_artifact_id,
        partial_output_ref: None,
        transport_attempt_id: None,
    }
}

/// An error a Provider reports inside its stream, read from the object holding it. A refusal
/// by a setting of the user's account is told apart, as it cannot pass when asked again; any
/// other stays a protocol failure under the Provider's own code, which decides that.
#[must_use]
pub fn provider_stream_failure(holder: &Value, fields: ProviderErrorFields) -> ModelAdapterError {
    let reply = ErrorReply::read(holder, fields);
    let message = reply.message.unwrap_or(fields.fallback_message);
    match account_setting(&reply.codes, message) {
        Some(setting) => ModelAdapterError {
            category: ModelFailureCategory::Authorization,
            code: setting.code().to_owned(),
            message: message.to_owned(),
        },
        None => ModelAdapterError::protocol(reply.code.unwrap_or(fields.fallback_code), message),
    }
}

#[derive(Default)]
struct ErrorReply<'a> {
    code: Option<&'a str>,
    codes: Vec<&'a str>,
    message: Option<&'a str>,
    names_parameter: bool,
}

impl<'a> ErrorReply<'a> {
    /// Reads the layouts Providers and their gateways answer with: an `error` object with its
    /// `message`, `Message` or `detail`, the same at the top level, an `error` string, or any of
    /// these as an array's first element.
    fn read(body: &'a Value, fields: ProviderErrorFields) -> Self {
        let body = body.as_array().and_then(|items| items.first()).unwrap_or(body);
        let error = body.get("error").filter(|error| error.is_object()).unwrap_or(body);
        let text = |field: &str| error.get(field).and_then(Value::as_str);
        Self {
            code: fields.code.iter().find_map(|field| text(field)),
            codes: fields
                .code
                .iter()
                .chain(&MATCHED_CODE_FIELDS)
                .filter_map(|field| text(field))
                .collect(),
            message: [
                error.get("message"),
                error.get("Message"),
                error.get("detail"),
                body.get("error"),
            ]
            .into_iter()
            .flatten()
            .find_map(Value::as_str),
            names_parameter: text("param").is_some(),
        }
    }
}
