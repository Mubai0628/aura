use super::*;

/// The fields of a failed reply. Under HTTP an error names its code as `code`, or as `type`.
pub(super) const HTTP_ERROR: ProviderErrorFields = ProviderErrorFields {
    code: &["code", "type"],
    fallback_code: "openai_responses_http_error",
    fallback_message: "OpenAI Responses request failed",
};

/// The fields of an `error` event or a failed response. The event's own `type` is the event's
/// name, not a code.
pub(super) const STREAM_ERROR: ProviderErrorFields = ProviderErrorFields {
    code: &["code"],
    fallback_code: "openai_responses_stream_error",
    fallback_message: "OpenAI Responses stream reported an error",
};
