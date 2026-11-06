//! The transport and the protocol adapters name no Provider's host: what a request to a host
//! carries beyond the protocol comes from the host's profile.
use std::path::Path;

use aura_provider_profile::{HostPattern, builtin_profiles};
use url::Url;

const CRATES: [&str; 4] = [
    "aura-model-transport",
    "aura-model-adapter-openai-responses",
    "aura-model-adapter-openai-chat",
    "aura-model-adapter-anthropic",
];

fn sources(directory: &Path, found: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(directory).expect("a source directory") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let text = std::fs::read_to_string(&path).expect("a source file");
            found.push((path.display().to_string(), text));
        }
    }
}

#[test]
fn the_transport_and_the_adapters_name_no_providers_host() {
    let mut hosts = Vec::new();
    for profile in builtin_profiles() {
        for rule in &profile.requests {
            if let HostPattern::Exact(name) | HostPattern::Domain(name) = &rule.target.host {
                hosts.push(name.clone());
            }
        }
        for route in &profile.routes {
            let url = Url::parse(&route.base_url).expect("a base URL");
            hosts.push(url.host_str().expect("a host").to_owned());
        }
    }
    hosts.sort();
    hosts.dedup();
    assert!(
        hosts.contains(&"api.openai.com".to_owned()) && hosts.contains(&"opencode.ai".to_owned())
    );
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the crates directory");
    let mut files = Vec::new();
    for name in CRATES {
        sources(&crates.join(name).join("src"), &mut files);
    }
    assert!(files.len() > CRATES.len());
    let named: Vec<String> = files
        .iter()
        .flat_map(|(path, text)| {
            hosts
                .iter()
                .filter(|host| text.contains(host.as_str()))
                .map(move |host| format!("{path}: {host}"))
        })
        .collect();
    assert!(named.is_empty(), "{named:#?}");
}
