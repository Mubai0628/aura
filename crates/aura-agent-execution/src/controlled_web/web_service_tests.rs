use super::*;

#[test]
fn a_loopback_or_private_address_stays_on_this_computer() {
    for local in [
        "http://localhost:3000/",
        "http://127.0.0.1/a",
        "http://192.168.1.20/",
        "http://10.0.0.5/",
        "http://[::1]/",
        "http://printer.local/",
        "http://intranet/",
    ] {
        assert!(local_address(&url::Url::parse(local).expect("url")), "{local}");
    }
    for public in ["https://example.com/", "https://8.8.8.8/", "https://api.deepseek.com/"] {
        assert!(!local_address(&url::Url::parse(public).expect("url")), "{public}");
    }
}

#[test]
fn only_deepseeks_own_api_over_https_carries_the_web_service() {
    for api in
        ["https://api.deepseek.com", "https://api.deepseek.com/v1", "https://API.DeepSeek.com/"]
    {
        assert!(deepseek_api(&url::Url::parse(api).expect("url")), "{api}");
    }
    for other in [
        "http://api.deepseek.com",
        "https://api.deepseek.com.example.net/",
        "https://example.net/api.deepseek.com",
        "https://deepseek.com/",
        "https://openrouter.ai/api/v1",
    ] {
        assert!(!deepseek_api(&url::Url::parse(other).expect("url")), "{other}");
    }
}

#[test]
fn search_and_fetch_take_the_models_input_in_deepseeks_shape() {
    let search = serde_json::json!({"query": "rust 2024 edition", "max_results": 80});
    assert_eq!(
        request_body("web.search", &search).expect("search"),
        serde_json::json!({"query": "rust 2024 edition", "count": 50}),
        "count is DeepSeek's 1 to 50"
    );
    let long = serde_json::json!({"query": "x".repeat(129), "max_results": 5});
    assert!(request_body("web.search", &long).is_err(), "more than 128 characters");
    let fetch =
        serde_json::json!({"url": "https://example.com/a", "mode": "auto", "max_bytes": 1024});
    assert_eq!(
        request_body("web.fetch", &fetch).expect("fetch"),
        serde_json::json!({"url": "https://example.com/a"})
    );
    let service = DeepSeekWebService {
        endpoint_id: EndpointId::new(),
        origin: "https://api.deepseek.com".into(),
        credential: None,
    };
    assert_eq!(
        service_url(&service, "web.search").expect("url").as_str(),
        "https://api.deepseek.com/v1/tools/search"
    );
    assert_eq!(
        service_url(&service, "web.fetch").expect("url").as_str(),
        "https://api.deepseek.com/v1/tools/fetch"
    );
}

#[test]
fn a_refusal_says_what_it_means_for_the_user() {
    assert!(refused(200, b"{}").is_ok());
    for (status, words) in [
        (401, "密钥"),
        (402, "余额"),
        (403, "安全策略"),
        (404, "http_only"),
        (429, "频繁"),
        (503, "暂时不可用"),
    ] {
        let message =
            refused(status, br#"{"error":{"message":"m"}}"#).expect_err("refused").to_string();
        assert!(message.contains(words), "{status}: {message}");
    }
}

/// DeepSeek's fetch is for pages. A fetch the model asks to make directly
/// (http_only), or of a file a page is made of or data is kept in, goes from this computer: DeepSeek
/// answered a three.js module with "404" and an API's JSON with escaped Markdown.
#[test]
fn a_fetch_asked_directly_or_of_a_file_goes_from_this_computer() {
    let input = |url: &str, mode: &str| -> WebFetchInput {
        serde_json::from_value(serde_json::json!({"url": url, "mode": mode})).expect("input")
    };
    for (url, mode) in [
        ("https://example.com/page", "http_only"),
        ("https://cdn.jsdelivr.net/npm/three@0.180.0/build/three.module.js", "auto"),
        ("https://example.com/data.json?page=2", "auto"),
        ("https://example.com/styles/site.css", "auto"),
        ("https://example.com/feed.xml", "auto"),
        ("http://127.0.0.1:5173/", "auto"),
    ] {
        let fetch = input(url, mode);
        assert!(fetched_here(&fetch).expect("url"), "{url} {mode}");
    }
    for url in
        ["https://example.com/", "https://example.com/article/42", "https://example.com/a.html"]
    {
        assert!(!fetched_here(&input(url, "auto")).expect("url"), "{url}");
    }
}

/// DeepSeek's fetch escapes the punctuation of a page's text as Markdown; a JSON answer came
/// back as `\{\"queryEnc\"\:…`. The escapes come off as a Markdown reader takes them, not in code.
#[test]
fn deepseeks_markdown_escapes_come_off_outside_code() {
    assert_eq!(
        markdown_unescaped(r#"\{\"queryEnc\"\:\"\%B7\"\,\"data\"\:\[1\]\}"#),
        r#"{"queryEnc":"%B7","data":[1]}"#
    );
    assert_eq!(markdown_unescaped(r"a \\ b \q c"), r"a \ b \q c", "only punctuation is escaped");
    assert_eq!(markdown_unescaped(r"use `a\_b` and \_c"), r"use `a\_b` and _c");
    let fenced = "```js\nconst a = \"\\{\";\n```\n\\{x\\}\n";
    assert_eq!(markdown_unescaped(fenced), "```js\nconst a = \"\\{\";\n```\n{x}\n");
}
