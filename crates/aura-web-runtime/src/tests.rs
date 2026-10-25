use super::*;
use aura_domain_common::ContentHash;
#[test]
fn search_extracts_only_results_preserves_query_and_decodes_entities() {
    let query = "fn main() { panic!(\"中文 & x<y\") }";
    let url = search_url_for(SearchProvider::DuckDuckGo, query, 4).expect("alternative query");
    assert_eq!(url.query_pairs().find(|(k, _)| k == "q").expect("query").1, query);
    let html = r#"<a href="/settings">Settings</a><a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fcode%3Fx%3D1%26y%3D2"><b>A &amp; B</b></a><a class="result__snippet">code &lt; T</a>"#;
    let hits = extract_search(&url, 200, "text/html", html.as_bytes(), 4).expect("results");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].url, "https://example.org/code?x=1&y=2");
    assert_eq!(hits[0].title, "A & B");
    assert_eq!(hits[0].snippet, "code < T");
}
#[test]
fn html_is_data_and_insufficient_auto_text_requires_rendering() {
    let url = normalize_url("https://example.org/page").expect("url");
    let html=b"<head><title>Page &amp; title</title><style>HIDDEN</style></head><body><script>if (a<b) { HIDDEN_SCRIPT(); }</script><p>Hello <b>world</b>.</p></body>";
    let document =
        extract_document(&url, 200, "text/html", html, WebFetchMode::HttpOnly).expect("document");
    assert_eq!(document.title, "Page & title");
    assert_eq!(document.text, "Hello world.");
    assert!(matches!(
        extract_document(&url, 200, "text/html", html, WebFetchMode::Auto),
        Err(WebError::RenderingUnavailable)
    ));
    assert!(matches!(
        extract_document(&url, 200, "text/html", html, WebFetchMode::HeadlessRenderRequired),
        Err(WebError::RenderingUnavailable)
    ));
    let rendered = document_from_rendered_html(
        &url,
        "<html><body><p>Rendered by isolated Chromium headless after the Gateway fetch.</p></body></html>",
        ContentHash::sha256(html),
    )
    .expect("rendered document");
    assert!(rendered.rendered);
    assert!(rendered.text.contains("isolated Chromium headless"));
}
#[test]
fn challenge_downloads_and_bad_urls_never_become_successful_documents() {
    let url = normalize_url("https://example.org").expect("url");
    assert!(matches!(
        extract_document(&url, 200, "application/octet-stream", b"MZ", WebFetchMode::HttpOnly),
        Err(WebError::UnsupportedMediaType)
    ));
    assert!(matches!(
        extract_document(
            &url,
            200,
            "text/html",
            b"<form id='challenge-form'>Solve</form>",
            WebFetchMode::HttpOnly
        ),
        Err(WebError::BotChallenge)
    ));
    assert!(normalize_url("http://user:pass@example.org/").is_err());
    assert!(normalize_url("file:///C:/private").is_err());
    assert!(matches!(
        extract_search(&url, 200, "text/html", b"<html>unexpected layout</html>", 5),
        Err(WebError::SearchSurfaceChanged)
    ));
}
#[test]
fn read_offsets_are_characters_and_out_of_range_is_rejected() {
    assert_eq!(excerpt("a中文🦀end", 1, 2).expect("read"), ("中文".into(), Some(3)));
    assert!(excerpt("a", 2, 2).is_err());
}
#[test]
fn executable_scripts_are_resolved_and_plain_type_is_ignored() {
    let url = normalize_url("https://example.org/dir/page").expect("url");
    let html = concat!(
        "<script src='app.js'></script>",
        "<script type='text/plain' src='skip.js'></script>",
        "<script src='https://cdn.example.net/x.js'></script>",
        "<script>inline()</script>"
    );
    let scripts = executable_script_resources(&url, html);
    assert_eq!(scripts.len(), 2);
    assert_eq!(scripts[0].src, "app.js");
    assert_eq!(scripts[0].url.as_str(), "https://example.org/dir/app.js");
    assert_eq!(scripts[1].url.as_str(), "https://cdn.example.net/x.js");
    let rewritten = rewrite_script_sources(html, &[("app.js".into(), "file:///staged.js".into())]);
    assert!(rewritten.contains("src='file:///staged.js'"));
    assert!(rewritten.contains("src='skip.js'"));
}

/// An API's JSON, a script, a
/// style sheet or a data file is text a model reads as it is, not a page to extract text from.
#[test]
fn text_that_is_not_a_page_is_kept_as_it_is() {
    let url = normalize_url("https://example.org/api/items").expect("url");
    for (media, body) in [
        ("application/json; charset=utf-8", r#"{"items":[{"id":1,"name":"a<b"}]}"#),
        ("application/javascript", "export const answer = 42;"),
        ("text/javascript", "import { x } from './x.js';"),
        ("text/css", ".a > .b { color: red; }"),
        ("text/csv", "id,name\n1,a\n"),
        ("application/xml", "<items><item id=\"1\"/></items>"),
        ("application/vnd.api+json", r#"{"data":[]}"#),
    ] {
        let document = extract_document(&url, 200, media, body.as_bytes(), WebFetchMode::Auto)
            .unwrap_or_else(|error| panic!("{media}: {error:?}"));
        assert_eq!(document.text, body, "{media}");
    }
    assert!(matches!(
        extract_document(&url, 200, "image/png", b"\x89PNG", WebFetchMode::Auto),
        Err(WebError::UnsupportedMediaType)
    ));
}
