#![forbid(unsafe_code)]
//! Stateless extraction of bounded, untrusted HTTP responses. Networking and
//! Artifact ownership remain Core responsibilities; no scripts are executed.
mod html;
mod search;
pub use search::{SearchProvider, search_url_for};
#[cfg(test)]
mod tests;
use aura_domain_common::{ContentHash, UtcTimestamp};
use aura_web_domain::{WebDocument, WebFetchMode, WebSearchHit};
use url::Url;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptResource {
    pub src: String,
    pub url: Url,
}

pub const SEARCH_SURFACE: &str = "bing-cn-html-v1";
pub const DOCUMENT_MEDIA_TYPE: &str = "application/vnd.aura.web-document+json";

#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("web.invalid_input")]
    InvalidInput,
    #[error("web.response_too_large")]
    ResponseTooLarge,
    #[error("web.authentication_required")]
    AuthenticationRequired,
    #[error("web.access_denied")]
    AccessDenied,
    #[error("web.bot_challenge")]
    BotChallenge,
    #[error("web.http_failed:{0}")]
    Http(u16),
    #[error("web.rendering_unavailable")]
    RenderingUnavailable,
    #[error("web.unsupported_media_type")]
    UnsupportedMediaType,
    #[error("web.search_surface_changed")]
    SearchSurfaceChanged,
}

pub fn normalize_url(value: &str) -> Result<Url, WebError> {
    let mut url = Url::parse(value).map_err(|_| WebError::InvalidInput)?;
    if value.len() > 32768
        || !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(WebError::InvalidInput);
    }
    url.set_fragment(None);
    Ok(url)
}

pub fn search_url(query: &str, max_results: u32) -> Result<Url, WebError> {
    search_url_for(SearchProvider::BingChina, query, max_results)
}

/// Text
/// that is not a page — plain text, Markdown, JSON, a script, a style sheet, CSV, XML or YAML — is
/// read as it is, never run and never mined for a page's text.
fn raw_text(media: &str) -> bool {
    let (kind, subtype) = media.split_once('/').unwrap_or((media, ""));
    matches!(
        media,
        "application/json"
            | "application/javascript"
            | "application/x-javascript"
            | "application/ecmascript"
            | "application/xml"
            | "application/yaml"
            | "application/x-yaml"
            | "application/toml"
            | "application/x-ndjson"
            | "application/csv"
    ) || (kind == "text" && subtype != "html")
        || (kind == "application"
            && subtype != "xhtml+xml"
            && (subtype.ends_with("+json") || subtype.ends_with("+xml")))
}

fn response_text(
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(String, String), WebError> {
    match status {
        401 | 407 => return Err(WebError::AuthenticationRequired),
        403 => return Err(WebError::AccessDenied),
        200..=299 => {}
        _ => return Err(WebError::Http(status)),
    }
    if body.len() > 8 * 1024 * 1024 {
        return Err(WebError::ResponseTooLarge);
    }
    let media = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if !matches!(media.as_str(), "text/html" | "application/xhtml+xml") && !raw_text(&media) {
        // Binary documents/downloads belong to a separate Artifact path. Never
        // interpret an executable response as page text or execute its content.
        return Err(WebError::UnsupportedMediaType);
    }
    let label = content_type.split(';').find_map(|part| {
        part.trim()
            .split_once('=')
            .filter(|(k, _)| k.trim().eq_ignore_ascii_case("charset"))
            .map(|(_, v)| v.trim().trim_matches(['\'', '"']))
    });
    let encoding = label
        .and_then(|v| encoding_rs::Encoding::for_label(v.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(body);
    Ok((media, text.into_owned()))
}

pub fn extract_document(
    url: &Url,
    status: u16,
    content_type: &str,
    body: &[u8],
    mode: WebFetchMode,
) -> Result<WebDocument, WebError> {
    if mode == WebFetchMode::HeadlessRenderRequired {
        return Err(WebError::RenderingUnavailable);
    }
    let (media, text) = response_text(status, content_type, body)?;
    let (title, text, links) = if raw_text(&media) {
        (url.as_str().to_owned(), text, vec![])
    } else {
        let page = html::extract(url, &text);
        if page.challenge {
            return Err(WebError::BotChallenge);
        }
        // Auto only succeeds with useful static text. It never silently turns
        // a rendering requirement into a successful empty document.
        if mode == WebFetchMode::Auto && page.text.chars().count() < 80 {
            return Err(WebError::RenderingUnavailable);
        }
        (page.title, page.text, page.links)
    };
    Ok(WebDocument {
        source_url: url.as_str().to_owned(),
        title,
        media_type: media,
        text,
        links,
        retrieved_at: UtcTimestamp::now(),
        response_hash: ContentHash::sha256(body),
        rendered: false,
    })
}

pub fn executable_script_resources(page_url: &Url, html: &str) -> Vec<ScriptResource> {
    html::script_refs(page_url, html)
        .into_iter()
        .filter(|script| script.executable && matches!(script.url.scheme(), "http" | "https"))
        .map(|script| ScriptResource { src: script.src, url: script.url })
        .collect()
}

pub fn rewrite_script_sources(html: &str, replacements: &[(String, String)]) -> String {
    let mut rewritten = html.to_string();
    for (from, to) in replacements {
        rewritten = rewritten.replace(&format!("src=\"{from}\""), &format!("src=\"{to}\""));
        rewritten = rewritten.replace(&format!("src='{from}'"), &format!("src='{to}'"));
    }
    rewritten
}

pub fn html_source(status: u16, content_type: &str, body: &[u8]) -> Result<String, WebError> {
    let (media, text) = response_text(status, content_type, body)?;
    if !matches!(media.as_str(), "text/html" | "application/xhtml+xml") {
        return Err(WebError::UnsupportedMediaType);
    }
    Ok(text)
}

pub fn document_from_rendered_html(
    url: &Url,
    rendered_html: &str,
    response_hash: ContentHash,
) -> Result<WebDocument, WebError> {
    if rendered_html.len() > 8 * 1024 * 1024 {
        return Err(WebError::ResponseTooLarge);
    }
    let page = html::extract(url, rendered_html);
    if page.challenge {
        return Err(WebError::BotChallenge);
    }
    Ok(WebDocument {
        source_url: url.as_str().to_owned(),
        title: page.title,
        media_type: "text/html".into(),
        text: page.text,
        links: page.links,
        retrieved_at: UtcTimestamp::now(),
        response_hash,
        rendered: true,
    })
}

pub fn extract_search(
    url: &Url,
    status: u16,
    content_type: &str,
    body: &[u8],
    max_results: u32,
) -> Result<Vec<WebSearchHit>, WebError> {
    if !(1..=20).contains(&max_results) {
        return Err(WebError::InvalidInput);
    }
    let (media, text) = response_text(status, content_type, body)?;
    if media != "text/html" {
        return Err(WebError::SearchSurfaceChanged);
    }
    search::extract(url, &text, max_results)
}

pub fn excerpt(text: &str, offset: u32, max_chars: u32) -> Result<(String, Option<u32>), WebError> {
    if !(1..=65536).contains(&max_chars) {
        return Err(WebError::InvalidInput);
    }
    let count = text.chars().count();
    if offset as usize > count {
        return Err(WebError::InvalidInput);
    }
    let value: String = text.chars().skip(offset as usize).take(max_chars as usize).collect();
    let next = offset.checked_add(value.chars().count() as u32).ok_or(WebError::InvalidInput)?;
    Ok((value, ((next as usize) < count).then_some(next)))
}
