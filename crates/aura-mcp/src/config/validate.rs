//! Checks of how a server is reached, in the user's words. Secrets never live in the
//! configuration: names that look like one are refused unless they say where Aura puts a
//! secret it keeps (a secret header or a secret environment variable).
use super::{
    MANAGED_HEADERS, MAX_ARGUMENTS, MAX_ENVIRONMENT, MAX_HEADERS, MAX_SCOPES,
    MAX_SECRET_ENVIRONMENT, MAX_VALUE_BYTES, McpHttpAuth, McpTransport, loopback, plain,
    secret_like,
};
use std::collections::BTreeSet;

fn environment_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn header_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}

/// Headers HTTP itself or cookies own; `Authorization` is allowed and then carries the secret
/// as its whole value (for example `Token …`), where no header means `Bearer <secret>`.
const REFUSED_SECRET_HEADERS: [&str; 9] = [
    "connection",
    "cookie",
    "set-cookie",
    "te",
    "upgrade",
    "trailer",
    "expect",
    "keep-alive",
    "via",
];

fn managed(name: &str) -> bool {
    MANAGED_HEADERS.iter().any(|managed| name.eq_ignore_ascii_case(managed))
}

pub(super) fn transport(transport: &McpTransport, problems: &mut Vec<String>) {
    match transport {
        McpTransport::Stdio {
            command,
            arguments,
            working_directory,
            environment,
            secret_environment,
        } => {
            if command.trim().is_empty() || !plain(command, 1024) {
                problems.push("command 不能为空，最长 1024 字节，不能含控制字符".into());
            }
            if arguments.len() > MAX_ARGUMENTS
                || arguments.iter().any(|a| a.contains('\0') || a.len() > MAX_VALUE_BYTES)
            {
                problems.push(format!(
                    "arguments 最多 {MAX_ARGUMENTS} 个，每个最长 {MAX_VALUE_BYTES} 字节"
                ));
            }
            if let Some(directory) = working_directory
                && (!std::path::Path::new(directory).is_absolute() || !plain(directory, 1024))
            {
                problems.push("working_directory 必须是完整的文件夹路径".into());
            }
            if environment.len() > MAX_ENVIRONMENT {
                problems.push(format!("environment 最多 {MAX_ENVIRONMENT} 项"));
            }
            let mut seen = BTreeSet::new();
            for entry in environment {
                if !environment_name(&entry.name) {
                    problems.push(format!("环境变量名 {} 不合法", entry.name));
                } else if secret_like(&entry.name) {
                    problems.push(format!(
                        "环境变量 {} 像是密钥。请不要把密钥写进配置，改用“带密钥的环境变量”",
                        entry.name
                    ));
                } else if !seen.insert(entry.name.to_ascii_uppercase()) {
                    problems.push(format!("环境变量 {} 重复", entry.name));
                }
                if entry.value.contains('\0') || entry.value.len() > MAX_VALUE_BYTES {
                    problems
                        .push(format!("环境变量 {} 的值最长 {MAX_VALUE_BYTES} 字节", entry.name));
                }
            }
            if secret_environment.len() > MAX_SECRET_ENVIRONMENT {
                problems.push(format!("带密钥的环境变量最多 {MAX_SECRET_ENVIRONMENT} 项"));
            }
            for entry in secret_environment {
                if !environment_name(&entry.name) {
                    problems.push(format!("环境变量名 {} 不合法", entry.name));
                } else if !seen.insert(entry.name.to_ascii_uppercase()) {
                    problems.push(format!("环境变量 {} 重复", entry.name));
                }
            }
            let secrets: BTreeSet<_> = secret_environment.iter().map(|e| e.secret_id).collect();
            if secrets.len() != secret_environment.len() {
                problems.push("同一个密钥只能给一个环境变量".into());
            }
        }
        McpTransport::Http { url, headers, auth } => {
            address(url, problems);
            if headers.len() > MAX_HEADERS {
                problems.push(format!("headers 最多 {MAX_HEADERS} 项"));
            }
            for header in headers {
                if !header_name(&header.name) {
                    problems.push(format!("请求头名 {} 不合法", header.name));
                } else if secret_like(&header.name) {
                    problems.push(format!(
                        "请求头 {} 带的是凭据。请不要把密钥写进配置，改在“认证”里设置密钥或 OAuth 登录",
                        header.name
                    ));
                } else if managed(&header.name) {
                    problems.push(format!("请求头 {} 由 Aura 设置，不能配置", header.name));
                }
                if !plain(&header.value, MAX_VALUE_BYTES) {
                    problems.push(format!("请求头 {} 的值不合法", header.name));
                }
            }
            match auth {
                None | Some(McpHttpAuth::Secret { header: None, .. }) => {}
                Some(McpHttpAuth::Secret { header: Some(name), .. }) => {
                    if !header_name(name) {
                        problems.push(format!("放密钥的请求头名 {name} 不合法"));
                    } else if managed(name)
                        || name.to_ascii_lowercase().starts_with("proxy-")
                        || REFUSED_SECRET_HEADERS
                            .iter()
                            .any(|refused| name.eq_ignore_ascii_case(refused))
                    {
                        problems.push(format!("请求头 {name} 不能用来放密钥"));
                    } else if headers.iter().any(|header| header.name.eq_ignore_ascii_case(name)) {
                        problems.push(format!("请求头 {name} 已经在普通请求头里"));
                    }
                }
                Some(McpHttpAuth::OAuth { client_id, scopes }) => {
                    if client_id.as_ref().is_some_and(|id| id.trim().is_empty() || !plain(id, 256))
                    {
                        problems.push("client_id 不能为空白，最长 256 字节".into());
                    }
                    let token = |scope: &String| {
                        !scope.is_empty()
                            && scope.len() <= 256
                            && scope
                                .bytes()
                                .all(|b| b > b' ' && b < 0x7f && b != b'"' && b != b'\\')
                    };
                    if scopes.len() > MAX_SCOPES || !scopes.iter().all(token) {
                        problems.push(format!(
                            "scopes 最多 {MAX_SCOPES} 个，每个不能含空格、引号或控制字符"
                        ));
                    }
                }
            }
        }
    }
}

fn address(url: &str, problems: &mut Vec<String>) {
    let Ok(parsed) = url::Url::parse(url) else {
        problems.push("url 不是合法的地址".into());
        return;
    };
    let host = parsed.host();
    let secure = parsed.scheme() == "https"
        || (parsed.scheme() == "http" && host.as_ref().is_some_and(loopback));
    if !secure {
        problems.push("url 必须是 https；只有本机地址可以用 http".into());
    }
    if host.is_none() || !parsed.username().is_empty() || parsed.password().is_some() {
        problems.push("url 不能含用户名或密码".into());
    }
    if parsed.fragment().is_some() || url.len() > 2048 {
        problems.push("url 不能含 #，最长 2048 字节".into());
    }
    if let Some((name, _)) = parsed.query_pairs().find(|(name, _)| secret_like(name)) {
        problems.push(format!(
            "url 的参数 {name} 像是密钥。请不要把密钥写进地址，改在“认证”里设置密钥或 OAuth 登录"
        ));
    }
}
