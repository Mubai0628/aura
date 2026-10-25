mod bing;
mod links;

use crate::WebError;
use aura_web_domain::WebSearchHit;
use url::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchProvider {
    BingChina,
    DuckDuckGo,
}

pub fn search_url_for(
    provider: SearchProvider,
    query: &str,
    max_results: u32,
) -> Result<Url, WebError> {
    if query.trim().is_empty()
        || query.len() > 8192
        || query.contains('\0')
        || !(1..=20).contains(&max_results)
    {
        return Err(WebError::InvalidInput);
    }
    let source = match provider {
        SearchProvider::BingChina => "https://cn.bing.com/search",
        SearchProvider::DuckDuckGo => "https://html.duckduckgo.com/html/",
    };
    let mut url = Url::parse(source).expect("fixed search surface");
    url.query_pairs_mut().append_pair("q", query);
    Ok(url)
}

#[derive(Default)]
pub(super) struct SearchPage {
    hits: Vec<WebSearchHit>,
    no_results: bool,
    challenge: bool,
}

pub(super) fn extract(
    url: &Url,
    text: &str,
    max_results: u32,
) -> Result<Vec<WebSearchHit>, WebError> {
    let page = match url.host_str() {
        Some("cn.bing.com" | "www.bing.com" | "bing.com") => bing::extract(url, text),
        Some("html.duckduckgo.com" | "duckduckgo.com" | "www.duckduckgo.com") => {
            let page = crate::html::extract(url, text);
            SearchPage { hits: page.hits, no_results: page.no_results, challenge: page.challenge }
        }
        _ => return Err(WebError::SearchSurfaceChanged),
    };
    if page.challenge {
        return Err(WebError::BotChallenge);
    }
    if page.hits.is_empty() && !page.no_results {
        return Err(WebError::SearchSurfaceChanged);
    }
    Ok(page.hits.into_iter().take(max_results as usize).collect())
}
