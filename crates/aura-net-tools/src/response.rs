use aura_network_control::OrdinaryGatewayResponse;
use aura_web_domain::*;

pub fn http_result(response: OrdinaryGatewayResponse, elapsed_ms: u64) -> HttpRequestResult {
    let r = response.response;
    let bytes_received = r.body.len() as u64;
    let content_type = r
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let is_json = content_type
        .split(';')
        .next()
        .is_some_and(|v| v.ends_with("/json") || v.ends_with("+json"));
    let text = std::str::from_utf8(&r.body).ok().filter(|v| !v.contains('\0'));
    let body_kind = if is_json {
        HttpBodyKind::Json
    } else if text.is_some() {
        HttpBodyKind::Text
    } else {
        HttpBodyKind::Binary
    };
    let truncated = r.body.len() > HTTP_INLINE_LIMIT;
    let inline = text.map(|v| {
        let mut end = v.len().min(HTTP_INLINE_LIMIT);
        while !v.is_char_boundary(end) {
            end -= 1;
        }
        v[..end].to_owned()
    });
    let json = if is_json && !truncated { serde_json::from_slice(&r.body).ok() } else { None };
    let hint = (truncated || text.is_none())
        .then(|| "Use http.download to save the complete response to an authorized file.".into());
    HttpRequestResult {
        status: r.status,
        headers: r
            .headers
            .into_iter()
            .filter(|(key, _)| !matches!(key.as_str(), "set-cookie" | "set-cookie2"))
            .map(|(name, value)| HttpHeader { name, value })
            .collect(),
        body_kind,
        text: if json.is_none() { inline } else { None },
        json,
        truncated,
        bytes_received,
        final_url: response.final_url,
        redirects: response.redirects,
        elapsed_ms,
        transfer_complete: true,
        hint,
    }
}
