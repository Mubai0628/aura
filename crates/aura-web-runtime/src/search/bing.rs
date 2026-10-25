use super::{SearchPage, links::bing_target};
use crate::html::{append, compact};
use aura_web_domain::WebSearchHit;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, states::RawKind,
};
use std::cell::RefCell;
use url::Url;

#[derive(Default)]
struct ResultBlock {
    nested_items: usize,
    heading: bool,
    paragraph: bool,
    url: Option<String>,
    title: String,
    snippet: String,
}

struct State {
    base: Url,
    page: SearchPage,
    result: Option<ResultBlock>,
    raw_skip: Option<String>,
}
struct Sink(RefCell<State>);

fn attr<'a>(tag: &'a Tag, name: &str) -> &'a str {
    tag.attrs
        .iter()
        .find(|attr| attr.name.local.as_ref() == name)
        .map(|attr| attr.value.as_ref())
        .unwrap_or("")
}

impl State {
    fn finish_result(&mut self) {
        let Some(result) = self.result.take() else {
            return;
        };
        let title = compact(&result.title);
        let Some(url) = result.url else {
            return;
        };
        if title.is_empty()
            || self.page.hits.len() >= 40
            || self.page.hits.iter().any(|hit| hit.url == url)
        {
            return;
        }
        self.page.hits.push(WebSearchHit {
            title,
            url,
            snippet: compact(&result.snippet),
            source: "public_page".into(),
            published_at: None,
        });
    }
}

impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) => {
                let name = tag.name.as_ref();
                let start = tag.kind == TagKind::StartTag;
                if let Some(skip) = &state.raw_skip {
                    if !start && skip == name {
                        state.raw_skip = None;
                    }
                    return TokenSinkResult::Continue;
                }
                if start && matches!(name, "script" | "style" | "iframe" | "noscript" | "xmp") {
                    state.raw_skip = Some(name.to_owned());
                    return TokenSinkResult::RawData(if name == "script" {
                        RawKind::ScriptData
                    } else {
                        RawKind::Rawtext
                    });
                }
                if start {
                    let classes = attr(&tag, "class").split_ascii_whitespace().collect::<Vec<_>>();
                    let marker = attr(&tag, "id");
                    if classes.contains(&"b_no") || marker == "b_no" {
                        state.page.no_results = true;
                    }
                    if matches!(marker, "b_captcha" | "b_challenge" | "challenge-form")
                        || classes.contains(&"b_captcha")
                    {
                        state.page.challenge = true;
                    }
                    if name == "li" {
                        if let Some(result) = state.result.as_mut() {
                            result.nested_items += 1;
                        } else if classes.contains(&"b_algo") {
                            state.result = Some(ResultBlock::default());
                        }
                    }
                } else if name == "li"
                    && let Some(result) = state.result.as_mut()
                {
                    if result.nested_items > 0 {
                        result.nested_items -= 1;
                    } else {
                        state.finish_result();
                    }
                }
                let base = state.base.clone();
                if let Some(result) = state.result.as_mut() {
                    if name == "h2" {
                        result.heading = start;
                    }
                    if start && name == "a" && result.heading && result.url.is_none() {
                        result.url = bing_target(&base, attr(&tag, "href"));
                    }
                    if name == "p" {
                        result.paragraph = start && result.snippet.is_empty();
                    }
                }
            }
            Token::CharacterTokens(text) if state.raw_skip.is_none() => {
                if let Some(result) = state.result.as_mut() {
                    if result.heading && result.url.is_some() {
                        append(&mut result.title, &text, 4096);
                    }
                    if result.paragraph {
                        append(&mut result.snippet, &text, 8192);
                    }
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

pub(super) fn extract(base: &Url, text: &str) -> SearchPage {
    let input = BufferQueue::default();
    input.push_back(text.into());
    let parser = Tokenizer::new(
        Sink(RefCell::new(State {
            base: base.clone(),
            page: SearchPage::default(),
            result: None,
            raw_skip: None,
        })),
        Default::default(),
    );
    let _ = parser.feed(&input);
    parser.end();
    let mut state = parser.sink.0.into_inner();
    state.finish_result();
    state.page
}
