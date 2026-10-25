use aura_web_domain::WebSearchHit;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, states::RawKind,
};
use std::cell::RefCell;
use url::Url;

#[derive(Default)]
pub(super) struct Page {
    pub title: String,
    pub text: String,
    pub links: Vec<WebSearchHit>,
    pub hits: Vec<WebSearchHit>,
    pub challenge: bool,
    pub no_results: bool,
}
struct Link {
    url: String,
    title: String,
    result: bool,
    snippet: bool,
}
struct State {
    page: Page,
    base: Url,
    raw_skip: Option<String>,
    title: bool,
    head: bool,
    link: Option<Link>,
}
struct Sink(RefCell<State>);

pub(super) fn append(to: &mut String, value: &str, bound: usize) {
    if to.len() < bound {
        let remaining = bound - to.len();
        let mut end = value.len().min(remaining);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        to.push_str(&value[..end]);
    }
}
pub(super) fn compact(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl State {
    fn finish_link(&mut self) {
        let Some(link) = self.link.take() else {
            return;
        };
        let title = compact(&link.title);
        if link.snippet {
            if let Some(last) = self.page.hits.last_mut() {
                last.snippet = title;
            }
            return;
        }
        if title.is_empty() || link.url.is_empty() {
            return;
        }
        let hit = WebSearchHit {
            title,
            url: link.url,
            snippet: String::new(),
            source: "public_page".into(),
            published_at: None,
        };
        if link.result
            && self.page.hits.len() < 40
            && !self.page.hits.iter().any(|v| v.url == hit.url)
        {
            self.page.hits.push(hit.clone());
        }
        if self.page.links.len() < 200 && !self.page.links.iter().any(|v| v.url == hit.url) {
            self.page.links.push(hit);
        }
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
                if name == "title" {
                    state.title = start;
                    if start {
                        return TokenSinkResult::RawData(RawKind::Rcdata);
                    }
                }
                if name == "head" {
                    state.head = start;
                }
                if start && (name == "form" || name == "div") {
                    let marker = tag
                        .attrs
                        .iter()
                        .filter(|v| matches!(v.name.local.as_ref(), "id" | "class" | "action"))
                        .map(|v| v.value.as_ref())
                        .collect::<Vec<_>>()
                        .join(" ");
                    if marker.contains("anomaly.js")
                        || marker.contains("challenge-form")
                        || marker.contains("cf-challenge")
                    {
                        state.page.challenge = true;
                    }
                    if marker.contains("no-results") {
                        state.page.no_results = true;
                    }
                }
                if name == "a" {
                    state.finish_link();
                    if start {
                        let attr = |key: &str| {
                            tag.attrs
                                .iter()
                                .find(|v| v.name.local.as_ref() == key)
                                .map(|v| v.value.as_ref())
                                .unwrap_or("")
                        };
                        let href = attr("href");
                        let mut target = state.base.join(href).ok();
                        if let Some(url) = &target
                            && matches!(
                                url.host_str(),
                                Some("duckduckgo.com" | "html.duckduckgo.com")
                            )
                            && url.path() == "/l/"
                        {
                            target = url
                                .query_pairs()
                                .find(|(k, _)| k == "uddg")
                                .and_then(|(_, v)| Url::parse(&v).ok());
                        }
                        let target = target
                            .and_then(|v| super::normalize_url(v.as_str()).ok())
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        let classes = attr("class").split_ascii_whitespace().collect::<Vec<_>>();
                        state.link = Some(Link {
                            url: target,
                            title: String::new(),
                            result: classes.contains(&"result__a"),
                            snippet: classes.contains(&"result__snippet"),
                        });
                    }
                }
                if matches!(
                    name,
                    "p" | "div" | "br" | "li" | "h1" | "h2" | "h3" | "article" | "section" | "tr"
                ) {
                    append(&mut state.page.text, "\n", 8 * 1024 * 1024);
                }
            }
            Token::CharacterTokens(text) if state.raw_skip.is_none() => {
                if state.title {
                    append(&mut state.page.title, &text, 4096);
                } else if !state.head {
                    append(&mut state.page.text, &text, 8 * 1024 * 1024);
                }
                if let Some(link) = state.link.as_mut() {
                    append(&mut link.title, &text, 8192);
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}
pub(super) fn extract(base: &Url, text: &str) -> Page {
    let input = BufferQueue::default();
    input.push_back(text.into());
    let parser = Tokenizer::new(
        Sink(RefCell::new(State {
            page: Page::default(),
            base: base.clone(),
            raw_skip: None,
            title: false,
            head: false,
            link: None,
        })),
        Default::default(),
    );
    let _ = parser.feed(&input);
    parser.end();
    let mut state = parser.sink.0.into_inner();
    state.finish_link();
    state.page.title = compact(&state.page.title);
    state.page.text = state
        .page
        .text
        .lines()
        .map(compact)
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    state.page
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScriptRef {
    pub src: String,
    pub url: Url,
    pub executable: bool,
}

struct ScriptState {
    base: Url,
    scripts: Vec<ScriptRef>,
}

struct ScriptSink(RefCell<ScriptState>);

fn executable_script_type(value: &str) -> bool {
    let media = value.trim().split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    matches!(
        media.as_str(),
        "" | "module"
            | "text/javascript"
            | "application/javascript"
            | "text/ecmascript"
            | "application/ecmascript"
    )
}

impl TokenSink for ScriptSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        if let Token::TagToken(tag) = token
            && tag.kind == TagKind::StartTag
            && tag.name.as_ref() == "script"
        {
            let attr = |key: &str| {
                tag.attrs
                    .iter()
                    .find(|v| v.name.local.as_ref() == key)
                    .map(|v| v.value.as_ref().to_owned())
            };
            let base = self.0.borrow().base.clone();
            if let Some(src) = attr("src")
                && !src.is_empty()
                && let Ok(url) = base.join(&src)
            {
                let executable = executable_script_type(attr("type").as_deref().unwrap_or(""));
                self.0.borrow_mut().scripts.push(ScriptRef { src, url, executable });
            }
            if !tag.self_closing {
                return TokenSinkResult::RawData(RawKind::ScriptData);
            }
        }
        TokenSinkResult::Continue
    }
}

pub(super) fn script_refs(base: &Url, text: &str) -> Vec<ScriptRef> {
    let input = BufferQueue::default();
    input.push_back(text.into());
    let parser = Tokenizer::new(
        ScriptSink(RefCell::new(ScriptState { base: base.clone(), scripts: Vec::new() })),
        Default::default(),
    );
    let _ = parser.feed(&input);
    parser.end();
    parser.sink.0.into_inner().scripts
}
