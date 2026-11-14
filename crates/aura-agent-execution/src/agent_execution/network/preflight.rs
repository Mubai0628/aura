use super::super::{AgentExecution, CallContext};
use aura_network_control::NetworkControlError;

impl AgentExecution {
    /// Explain a refused target before selection can collapse it into backend availability.
    pub(in crate::agent_execution) async fn network_preflight_notice(
        &self,
        call: &CallContext,
    ) -> Option<String> {
        let content = &call.operation.content;
        if !aura_security_contract::controlled_tool_network(
            &content.source,
            &content.action_kind,
            &content.capabilities,
            &content.resources,
        ) {
            return None;
        }
        self.web.preflight_network_before_answer(&call.operation).await.err().map(|e| notice(&e))
    }
}

/// The notice starts with one of the codes the stored result classifies; the rest says why the
/// target was refused. Every line here is Core's own: transport failures arrive already worded
/// without the library's or the system's text.
fn notice(error: &anyhow::Error) -> String {
    let text = error.to_string();
    if matches!(
        error.downcast_ref::<NetworkControlError>(),
        Some(NetworkControlError::SecretDetected { .. })
    ) {
        if text.starts_with("http.model_authentication_forbidden:") {
            return text;
        }
        return "http.model_authentication_forbidden: 认证信息只能通过工具凭据提供（工具设置里配置凭据与允许网站）。尚未发出请求。".to_owned();
    }
    if text.starts_with("http.credential_rebind_required:") {
        return text;
    }
    match text.split(':').next().unwrap_or_default() {
        "network.control_endpoint_blocked" => "network.control_endpoint_blocked: 目标是 Aura 正在使用的控制端点，不能向该地址发送工具请求。请改用项目 API 地址。".to_owned(),
        "network.timeout" => "network.timeout: 目标地址解析超时，尚未发出请求。请检查地址与本地服务后重新调用。".to_owned(),
        _ => format!("network.request_refused: 目标地址没有通过检查，尚未发出请求。{}", cause(&text)),
    }
}

/// The first line of Core's reason: at most 400 bytes, without control characters.
fn cause(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.is_empty() {
        return "请检查 URL、DNS 与服务状态。".to_owned();
    }
    let mut cause = String::new();
    for c in line.chars().map(|c| if c.is_control() { ' ' } else { c }) {
        if cause.len() + c.len_utf8() > 400 {
            cause.push('…');
            break;
        }
        cause.push(c);
    }
    cause
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text the controlled web gives a gateway refusal that took no proxy route.
    fn refused(error: NetworkControlError) -> anyhow::Error {
        anyhow::anyhow!(aura_network_control::ordinary_tools::safe_transport_line(&error))
    }

    /// A refused target keeps the code the result classifies, and says the cause it was refused
    /// for instead of one sentence for every cause.
    #[test]
    fn a_refused_target_names_its_cause() {
        let blocked = notice(&refused(NetworkControlError::ControlEndpointBlocked));
        assert!(blocked.starts_with("network.control_endpoint_blocked: 目标是 Aura"), "{blocked}");
        let timeout = notice(&refused(NetworkControlError::Timeout));
        assert!(timeout.starts_with("network.timeout: 目标地址解析超时"), "{timeout}");
        let invalid = notice(&refused(NetworkControlError::InvalidUrl));
        assert!(invalid.starts_with("network.request_refused: 目标地址没有通过检查"), "{invalid}");
        assert!(invalid.contains("http.invalid_url_or_method: 地址不是有效的"), "{invalid}");
        let secret = notice(&anyhow::Error::new(NetworkControlError::SecretDetected {
            location: "header.authorization".into(),
            kind: "credential_header".into(),
        }));
        assert!(secret.starts_with("http.model_authentication_forbidden: "), "{secret}");
        let long = cause(&format!("x{}\nsecond line", "中".repeat(400)));
        assert!(long.len() <= 403 && long.ends_with('…') && !long.contains("second"), "{long}");
    }
}
