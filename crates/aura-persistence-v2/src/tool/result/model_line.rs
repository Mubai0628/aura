//! The lines of a failed call the model reads in Core's own words: a web, download or request
//! failure whose first line is Core's sentence, and why a call did not start.
use super::super::refusal_text;
use aura_domain_common::ErrorCode;
use aura_tool_contract::ToolNetworkErrorCode;
use serde_json::Value;

pub(super) fn model_safe_named_error(raw: &str) -> Option<&str> {
    let first = raw.lines().next()?.trim();
    let code = first.split(':').next()?.trim();
    if code.is_empty() || code.len() > 128 {
        return None;
    }
    ErrorCode::new(code).ok()?;
    // Gateway diagnostics after the code stay hidden. Core's proxy sentences say whether
    // the HTTP request was sent, and those are the lines the model is meant to read.
    let clean = !first.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'));
    let visible = clean
        && (first.len() <= 512
            && (proxy_explanation_for_the_model(first)
                || core_web_service_line(code)
                || code.starts_with("http.download_folder_")
                || unreachable_target(code).is_some())
            // Core's own sentences carry a bounded detail, in Chinese for most families.
            || first.len() <= 1024 && core_sentence_code(code));
    Some(if visible { first } else { code })
}

/// Codes whose line is always Core's own sentence: what happened, whether the request went out,
/// and what to do next. A code the gateway's or a library's text can also carry
/// (`network.transport_failed`, `network.quarantine_io_failed`) is not here.
fn core_sentence_code(code: &str) -> bool {
    const WEB: &[&str] = &[
        "invalid_input",
        "response_too_large",
        "authentication_required",
        "access_denied",
        "bot_challenge",
        "rendering_unavailable",
        "unsupported_media_type",
        "search_surface_changed",
        "session_required",
        "agent_authority_required",
        "preparation_changed",
        "mode_changed_before_start",
        "document_limit",
        "document_missing",
        "workspace_scope",
        "origin_binding",
        "byte_limit",
        "action_unsupported",
        "fetch_input",
        "search_input",
        "read_input",
    ];
    const HTTP: &[&str] = &[
        // Core's transport lines.
        "invalid_url_or_method",
        "model_authentication_forbidden",
        "credential_unavailable",
        "authority_changed",
        // The steps of a download.
        "download_status",
        "partial_response",
        "staging_poisoned",
        "staging_finished",
        "staging_write_failed",
        "staging_failed",
        "never_published",
        "publication_rolled_back",
        "publication_unconfirmed",
        "target_parent",
        "target_leaf",
        "target_exists",
        "target_not_file",
        "target_unusable",
        "target_changed",
        "parent_changed",
        "inconsistent_target",
        "target_busy",
        "download_not_started",
        "resolved_target_changed",
        "path_required",
        "target_authority_missing",
        // The checks of a request before it goes out.
        "invalid_url",
        "invalid_method",
        "unsupported_method",
        "body_not_supported",
        "header_limit",
        "invalid_header_name",
        "invalid_header_value",
        "header_controlled",
        "body_limit",
        "invalid_content_type",
        // Credentials and arguments.
        "unavailable",
        "credential_unbound",
        "credential_unbound_or_ambiguous",
        "credential_origin_denied",
        "credential_changed",
        "credential_rebind_required",
        "credential_placement_invalid",
        "url_required",
        "host",
        "port",
        "unsupported_action",
        "action_object_required",
        "request_input",
        "download_input",
    ];
    const NETWORK: &[&str] = &[
        "control_endpoint_blocked",
        "response_too_large",
        "redirect_blocked",
        "credential_reflection_blocked",
        "rate_limited",
        "request_refused",
    ];
    const PACKAGE: &[&str] =
        &["http_result_invalid", "https_required", "origin_missing", "preparation_failed"];
    const AGENT: &[&str] = &[
        "permission_withdrawn",
        "plan_call_missing",
        "web_unsupported",
        "web_binding_changed",
        "web_no_claim",
    ];
    let Some((family, name)) = code.split_once('.') else { return false };
    match family {
        "web" => WEB.contains(&name),
        "http" => HTTP.contains(&name),
        "network" => NETWORK.contains(&name) || name.starts_with("user_proxy_"),
        "package" => PACKAGE.contains(&name) || name.starts_with("http_status_"),
        "agent" => AGENT.contains(&name),
        _ => false,
    }
}

/// A target Core could not reach, by kind, or one that did not answer in time: its first line is
/// Core's own sentence (host, port and what to do next), and every kind but a reset or a timeout
/// left the request unsent.
pub(super) fn unreachable_target(code: &str) -> Option<(ToolNetworkErrorCode, bool)> {
    Some(match code {
        "network.connection_refused" => (ToolNetworkErrorCode::ConnectionRefused, true),
        "network.host_not_found" => (ToolNetworkErrorCode::HostNotFound, true),
        "network.tls_failed" => (ToolNetworkErrorCode::TlsFailed, true),
        "network.connection_reset" => (ToolNetworkErrorCode::ConnectionReset, false),
        "network.connection_failed" => (ToolNetworkErrorCode::ConnectionFailed, true),
        "network.timeout" => (ToolNetworkErrorCode::TimedOut, false),
        _ => return None,
    })
}

/// The first line of a DeepSeek web service refusal is Core's own sentence, and so is the line of
/// a status a site answered (the number and what it means).
fn core_web_service_line(code: &str) -> bool {
    code.starts_with("web.deepseek_")
        || code == "web.service_connection_unusable"
        || code == "web.search_query_too_long"
        || code == "web.http_failed"
}

fn proxy_explanation_for_the_model(line: &str) -> bool {
    line.contains("尚未向目标发出 HTTP 请求") || line.contains("不能确认目标请求是否已经发出")
}

/// Families whose prepared rejections are their own sentences, with the cause they found.
const OWN_WORDS: &[&str] = &[
    "compat.",
    "git.",
    "terminal.",
    "command.",
    "package.",
    "browser.",
    "chrome.",
    "computer.",
    "media.",
    "memory.",
    "mcp.",
];

/// Why a call did not start, from its refusal's own reason: a known reason in words, or a
/// family's own `code: sentence`. Nothing else of the reason (an operation identity, an internal
/// message) reaches the model; an unknown reason is named by its code only.
pub(super) fn not_started_message(evidence: &Value) -> String {
    let reason = if evidence["schema"] == "execution-prepared-rejection-v1" {
        evidence["reason"].as_str().unwrap_or_default()
    } else {
        ""
    };
    // The shell, Git, the terminal and the families that word their own refusals say more than
    // the table's general sentence for their codes.
    let own = refusal_text::readable_line(reason)
        .filter(|line| OWN_WORDS.iter().any(|family| line.starts_with(family)));
    if let Some(line) = own {
        return line.to_owned();
    }
    if let Some(sentence) = refusal_text::known(reason) {
        return sentence.to_owned();
    }
    if let Some(line) = refusal_text::readable_line(reason) {
        return line.to_owned();
    }
    if evidence["schema"] == "windows-user-execution-evidence-v2" {
        return "This command was not started: the run stopped before it began. Nothing was changed. Run it again if it is still needed."
            .to_owned();
    }
    let named = refusal_text::code_in(reason).map(|code| format!(" ({code})"));
    format!(
        "This call was not started: it was refused before it could run{}. Nothing was changed. Check the current state, then call it again if it is still needed.",
        named.unwrap_or_default()
    )
}
