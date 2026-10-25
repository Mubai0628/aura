use aura_web_runtime::{SEARCH_SURFACE, WebError, extract_search, search_url};
use url::Url;

const BING: &[u8] = include_bytes!("fixtures/bing-results.html");

#[test]
fn search_defaults_to_china_bing_and_preserves_the_literal_query() {
    let query = "Rust Option 中文 & x<y";
    let url = search_url(query, 4).expect("valid search query");
    assert_eq!(url.origin().ascii_serialization(), "https://cn.bing.com");
    assert_eq!(url.path(), "/search");
    assert_eq!(url.query_pairs().find(|(key, _)| key == "q").expect("query pair").1, query);
    assert_eq!(SEARCH_SURFACE, "bing-cn-html-v1");
}

#[test]
fn saved_bing_results_extract_titles_real_links_and_snippets() {
    let url = Url::parse("https://cn.bing.com/search?q=Rust+Option").expect("sample source");
    let hits = extract_search(&url, 200, "text/html; charset=utf-8", BING, 10)
        .expect("saved Bing results");
    assert_eq!(hits.len(), 2, "navigation, ads, unsafe and duplicate links are excluded");
    assert_eq!(hits[0].title, "Option & Result — Rust");
    assert_eq!(hits[0].url, "https://doc.rust-lang.org/std/option/index.html");
    assert_eq!(hits[0].snippet, "Handle optional values & errors.");
    assert_eq!(hits[1].url, "https://example.org/tutorial?x=1&y=2");
    assert_eq!(hits[1].snippet, "中文 snippet with nested text.");
    assert_eq!(extract_search(&url, 200, "text/html", BING, 1).expect("bounded search").len(), 1);
}

#[test]
fn bing_empty_results_challenges_and_changed_markup_remain_distinct() {
    let url = Url::parse("https://cn.bing.com/search?q=none").expect("sample source");
    assert!(
        extract_search(&url, 200, "text/html", include_bytes!("fixtures/bing-empty.html"), 4)
            .expect("explicit no-results page")
            .is_empty()
    );
    assert!(matches!(
        extract_search(&url, 200, "text/html", include_bytes!("fixtures/bing-challenge.html"), 4),
        Err(WebError::BotChallenge)
    ));
    assert!(matches!(
        extract_search(&url, 200, "text/html", b"<html>unknown layout</html>", 4),
        Err(WebError::SearchSurfaceChanged)
    ));
}

#[test]
fn saved_duckduckgo_alternative_remains_supported() {
    let url =
        aura_web_runtime::search_url_for(aura_web_runtime::SearchProvider::DuckDuckGo, "Rust", 4)
            .expect("alternative source");
    let html = br#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2F"><b>Rust &amp; tools</b></a><a class="result__snippet">Alternative snippet</a>"#;
    let hits = extract_search(&url, 200, "text/html", html, 4).expect("alternative parser");
    assert_eq!(hits[0].url, "https://example.org/");
    assert_eq!(hits[0].snippet, "Alternative snippet");
}

#[test]
fn the_single_saved_live_sample_extracts_organic_results_without_network_access() {
    let url = search_url("Rust Option documentation", 4).expect("sample query");
    let hits = extract_search(
        &url,
        200,
        "text/html",
        include_bytes!("fixtures/bing-sampled-results.html"),
        4,
    )
    .expect("saved real layout");
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].title, "Rust 程序设计语言");
    assert_eq!(hits[0].url, "https://rust-lang.org/zh-CN/");
    assert!(hits[0].snippet.contains("生产环境中的 Rust"));
    assert_eq!(hits[1].title, "Rust 教程 | 菜鸟教程");
    assert_eq!(hits[1].url, "https://www.runoob.com/rust/rust-tutorial.html");
    assert!(hits[1].snippet.contains("安全、并发、实用"));
}
