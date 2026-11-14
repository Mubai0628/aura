//! The shared site-rule cases (`fixtures/browser/site-rules.v1.json`): the browser host runs the
//! same file, so a rule the user writes means the same thing in Core and in the host.
use aura_browser_domain::{
    BrowserSiteDecision, BrowserSiteRule, BrowserSiteRuleError, site_of_url,
};
use serde_json::Value;

fn cases() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/browser/site-rules.v1.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json")
}

#[test]
fn rules_parse_to_their_canonical_text_or_name_what_is_wrong() {
    for case in cases()["parse"].as_array().expect("parse cases") {
        let input = case["input"].as_str().expect("input");
        let parsed = BrowserSiteRule::parse(input);
        match (case.get("rule"), case.get("error")) {
            (Some(rule), None) => assert_eq!(
                parsed.as_ref().map(ToString::to_string).ok().as_deref(),
                rule.as_str(),
                "{input:?} -> {parsed:?}"
            ),
            (None, Some(error)) => {
                let kind = match parsed {
                    Err(BrowserSiteRuleError::Empty) => "empty",
                    Err(BrowserSiteRuleError::Host) => "host",
                    Err(BrowserSiteRuleError::Port) => "port",
                    Err(BrowserSiteRuleError::Scheme) => "scheme",
                    Ok(rule) => panic!("{input:?} must be refused, got {rule}"),
                };
                assert_eq!(kind, error.as_str().expect("error"), "{input:?}");
            }
            _ => panic!("bad case {case}"),
        }
    }
}

#[test]
fn rules_match_hosts_subdomains_and_ports_exactly_as_written() {
    for case in cases()["match"].as_array().expect("match cases") {
        let rule =
            BrowserSiteRule::parse(case["rule"].as_str().expect("rule")).expect("valid rule");
        let url = case["url"].as_str().expect("url");
        let site = site_of_url(url).unwrap_or_else(|| panic!("{url} has a site"));
        assert_eq!(
            rule.matches(&site),
            case["matches"].as_bool().expect("matches"),
            "{rule} {url}"
        );
    }
}

#[test]
fn pages_without_a_network_site_have_none() {
    for url in cases()["no_site"].as_array().expect("no_site") {
        let url = url.as_str().expect("url");
        assert_eq!(site_of_url(url), None, "{url}");
    }
}

#[test]
fn a_blocked_site_stays_blocked_even_when_also_allowed() {
    let allowed = [BrowserSiteRule::parse("*.example.com").expect("rule")];
    let blocked = [BrowserSiteRule::parse("ads.example.com").expect("rule")];
    let decide =
        |url: &str| BrowserSiteDecision::of(&site_of_url(url).expect("site"), &allowed, &blocked);
    assert_eq!(decide("https://ads.example.com/x"), BrowserSiteDecision::Blocked);
    assert_eq!(decide("https://www.example.com/x"), BrowserSiteDecision::Allowed);
    assert_eq!(decide("https://other.net/"), BrowserSiteDecision::Unlisted);
}
