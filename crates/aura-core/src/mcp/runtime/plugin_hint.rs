//! A plugin's server that wants a sign-in but declares none: its error says how to give it one.
use aura_mcp::{McpServerConfig, McpTransport};

/// The error to show for a server: unchanged, except for a plugin's HTTP server that declares no
/// sign-in and was refused for wanting one.
pub(super) fn rewrite(
    plugin: Option<&str>,
    server: &McpServerConfig,
    last_error: Option<String>,
) -> Option<String> {
    let declares_no_sign_in = matches!(server.transport, McpTransport::Http { auth: None, .. });
    match (plugin, last_error) {
        (Some(plugin), Some(error))
            if declares_no_sign_in && error.contains(crate::mcp::http::SIGN_IN_REQUIRED) =>
        {
            Some(format!(
                "握手失败：服务器要求登录（HTTP 401）。插件 {plugin} 没有为这个服务器声明登录方式：点“编辑”，在认证里选择登录方式并填写密钥，保存后重新信任。"
            ))
        }
        (_, last_error) => last_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::http::SIGN_IN_REQUIRED;
    use aura_domain_common::CredentialId;
    use aura_mcp::McpHttpAuth;

    fn server(auth: Option<McpHttpAuth>) -> McpServerConfig {
        McpServerConfig {
            id: "github".to_owned(),
            display_name: "GitHub".to_owned(),
            transport: McpTransport::Http {
                url: "https://api.githubcopilot.com/mcp/".to_owned(),
                headers: Vec::new(),
                auth,
            },
            enabled: true,
            trusted_fingerprint: None,
            startup_timeout_ms: 30_000,
            call_timeout_ms: 120_000,
            available_for_subagents: false,
        }
    }

    fn refused() -> Option<String> {
        Some(format!("握手失败：{SIGN_IN_REQUIRED}，请在服务器设置的“认证”里设置密钥或 OAuth 登录"))
    }

    #[test]
    fn a_plugin_server_without_a_sign_in_points_at_its_settings() {
        let shown = rewrite(Some("github-tools"), &server(None), refused()).expect("an error");
        assert!(shown.contains("插件 github-tools 没有为这个服务器声明登录方式"), "{shown}");
        assert!(shown.contains("点“编辑”，在认证里选择登录方式并填写密钥"), "{shown}");
        assert!(shown.contains("重新信任"), "{shown}");
    }

    #[test]
    fn everything_else_keeps_its_own_error() {
        // The user's own server has server settings to fix it in.
        assert_eq!(rewrite(None, &server(None), refused()), refused());
        // A plugin server that declared a secret is asked for it in its row already.
        let secret = McpHttpAuth::Secret { secret_id: CredentialId::new(), header: None };
        assert_eq!(rewrite(Some("p"), &server(Some(secret)), refused()), refused());
        // Another failure is not about signing in.
        let other = Some("握手失败：连接被拒绝".to_owned());
        assert_eq!(rewrite(Some("p"), &server(None), other.clone()), other);
        assert_eq!(rewrite(Some("p"), &server(None), None), None);
    }
}
