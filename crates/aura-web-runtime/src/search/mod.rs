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

/// The rule a search's result count breaks.
pub(crate) const MAX_RESULTS_RULE: &str =
    "max_results must be between 1 and 20. Nothing was searched.";

pub fn search_url_for(
    provider: SearchProvider,
    query: &str,
    max_results: u32,
) -> Result<Url, WebError> {
    let broken = if query.trim().is_empty() {
        Some("the search query is empty; give the words to search for. Nothing was searched.")
    } else if query.len() > aura_web_domain::LOCAL_SEARCH_MAX_QUERY_BYTES {
        Some("the search query is longer than 8192 bytes; shorten it. Nothing was searched.")
    } else if query.contains('\0') {
        Some("the search query contains a NUL character; remove it. Nothing was searched.")
    } else if !(1..=aura_web_domain::LOCAL_SEARCH_MAX_RESULTS).contains(&max_results) {
        Some(MAX_RESULTS_RULE)
    } else {
        None
    };
    if let Some(rule) = broken {
        return Err(WebError::invalid(rule));
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
