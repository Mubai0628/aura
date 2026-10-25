#![forbid(unsafe_code)]

//! Shared credential pattern detector. Returns only a detection class; never the value.
use regex::Regex;
use std::sync::OnceLock;

pub fn detect_text(value: &str) -> Option<&'static str> {
    if value.contains("-----BEGIN PRIVATE KEY-----")
        || value.contains("-----BEGIN RSA PRIVATE KEY-----")
        || value.contains("-----BEGIN OPENSSH PRIVATE KEY-----")
    {
        return Some("private_key");
    }
    let patterns = patterns();
    if patterns.github.is_match(value) {
        return Some("github_token");
    }
    if patterns.openai.is_match(value) {
        return Some("api_token");
    }
    if patterns.aws_access_key.is_match(value) {
        return Some("aws_access_key");
    }
    if patterns.jwt.is_match(value) {
        return Some("jwt");
    }
    if patterns.assignment.is_match(value) {
        return Some("credential_assignment");
    }
    None
}

struct Patterns {
    github: Regex,
    openai: Regex,
    aws_access_key: Regex,
    jwt: Regex,
    assignment: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        github: Regex::new(r"\b(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,})\b")
            .expect("static GitHub token regex"),
        openai: Regex::new(r"\bsk-[A-Za-z0-9_-]{20,}\b").expect("static API token regex"),
        aws_access_key: Regex::new(r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b")
            .expect("static AWS key regex"),
        jwt: Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b")
            .expect("static JWT regex"),
        assignment: Regex::new(
            r"(?i)\b(?:api[_-]?key|access[_-]?token|password|passwd|client[_-]?secret)\s*[:=]\s*[^\s,;]{8,}",
        )
        .expect("static credential assignment regex"),
    })
}
