/// Authentication headers are refused in model input and stripped across origins.
#[must_use]
pub fn is_authentication_header(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "cookie2"
            | "x-api-key"
            | "api-key"
            | "x-auth-token"
            | "x-access-token"
    )
}
