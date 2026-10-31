use super::super::{AgentExecution, CallContext};

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
        self.web.preflight_network_before_answer(&call.operation).await.err().map(|error| {
            if matches!(error.downcast_ref::<aura_network_control::NetworkControlError>(), Some(aura_network_control::NetworkControlError::SecretDetected { .. })) {
                return "http.model_authentication_forbidden: 认证信息只能通过工具凭据提供（工具设置里配置凭据与允许网站）。尚未发出请求。".to_owned();
            }
            if error.to_string().starts_with("http.credential_rebind_required:") { return error.to_string(); }
            match error.to_string().as_str() {
                "network.control_endpoint_blocked"=>"network.control_endpoint_blocked: 目标是 Aura 正在使用的控制端点，不能向该地址发送工具请求。请改用项目 API 地址。".to_owned(),
                "network.timeout"=>"network.timeout: 目标地址解析超时，尚未发出请求。请检查地址与本地服务后重新调用。".to_owned(),
                _=>"network.request_refused: 无法验证目标地址，尚未发出请求。请检查 URL、DNS 与服务状态。".to_owned(),
            }
        })
    }
}
