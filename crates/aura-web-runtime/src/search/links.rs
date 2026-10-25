use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use url::Url;

pub(super) fn bing_target(base: &Url, href: &str) -> Option<String> {
    let url = base.join(href).ok()?;
    let target = if matches!(url.host_str(), Some("cn.bing.com" | "www.bing.com" | "bing.com"))
        && url.path() == "/ck/a"
    {
        let encoded = url.query_pairs().find(|(key, _)| key == "u")?.1;
        let encoded = encoded.strip_prefix("a1")?;
        let bytes = [URL_SAFE_NO_PAD, URL_SAFE, STANDARD_NO_PAD, STANDARD]
            .iter()
            .find_map(|engine| engine.decode(encoded).ok())?;
        String::from_utf8(bytes).ok()?
    } else {
        url.to_string()
    };
    crate::normalize_url(&target).ok().map(|url| url.to_string())
}
