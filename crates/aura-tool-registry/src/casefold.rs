use std::{collections::BTreeMap, sync::OnceLock};

use aura_domain_common::ContentHash;
use thiserror::Error;

pub const UNICODE_CASE_FOLDING_VERSION_V1: &str = "17.0.0";
pub const UNICODE_CASE_FOLDING_SHA256_V1: &str =
    "ff8d8fefbf123574205085d6714c36149eb946d717a0c585c27f0f4ef58c4183";

const CASE_FOLDING_SOURCE: &[u8] =
    include_bytes!("../../../resources/unicode/17.0.0/CaseFolding.txt");

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum UnicodeCaseFoldError {
    #[error("the embedded Unicode 17.0 CaseFolding source hash is invalid")]
    SourceHashMismatch,
    #[error("the embedded Unicode 17.0 CaseFolding source is malformed")]
    SourceMalformed,
}

#[derive(Debug)]
struct UnicodeCaseFoldTable {
    mappings: BTreeMap<char, String>,
}

static TABLE: OnceLock<Result<UnicodeCaseFoldTable, UnicodeCaseFoldError>> = OnceLock::new();

pub fn unicode_default_casefold_v1(value: &str) -> Result<String, UnicodeCaseFoldError> {
    let table = TABLE.get_or_init(parse_embedded_table).as_ref().map_err(Clone::clone)?;
    let mut folded = String::with_capacity(value.len());
    for character in value.chars() {
        if let Some(mapping) = table.mappings.get(&character) {
            folded.push_str(mapping);
        } else {
            folded.push(character);
        }
    }
    Ok(folded)
}

fn parse_embedded_table() -> Result<UnicodeCaseFoldTable, UnicodeCaseFoldError> {
    if ContentHash::sha256(CASE_FOLDING_SOURCE).hex != UNICODE_CASE_FOLDING_SHA256_V1 {
        return Err(UnicodeCaseFoldError::SourceHashMismatch);
    }
    let source = std::str::from_utf8(CASE_FOLDING_SOURCE)
        .map_err(|_| UnicodeCaseFoldError::SourceMalformed)?;
    let mut mappings = BTreeMap::new();
    for raw_line in source.lines() {
        let data = raw_line.split('#').next().unwrap_or_default().trim();
        if data.is_empty() {
            continue;
        }
        let fields = data.split_terminator(';').map(str::trim).collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err(UnicodeCaseFoldError::SourceMalformed);
        }
        if !matches!(fields[1], "C" | "F") {
            continue;
        }
        let source = parse_scalar(fields[0])?;
        let mapping =
            fields[2].split_ascii_whitespace().map(parse_scalar).collect::<Result<String, _>>()?;
        if mapping.is_empty() || mappings.insert(source, mapping).is_some() {
            return Err(UnicodeCaseFoldError::SourceMalformed);
        }
    }
    if mappings.is_empty() {
        return Err(UnicodeCaseFoldError::SourceMalformed);
    }
    Ok(UnicodeCaseFoldTable { mappings })
}

fn parse_scalar(value: &str) -> Result<char, UnicodeCaseFoldError> {
    u32::from_str_radix(value, 16)
        .ok()
        .and_then(char::from_u32)
        .ok_or(UnicodeCaseFoldError::SourceMalformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_default_non_turkic_case_folding_uses_the_pinned_unicode_table() {
        assert_eq!(unicode_default_casefold_v1("FUSS").expect("fold"), "fuss");
        assert_eq!(unicode_default_casefold_v1("Fuß").expect("fold"), "fuss");
        assert_eq!(unicode_default_casefold_v1("İ").expect("fold"), "i\u{307}");
        assert_eq!(unicode_default_casefold_v1("Iı").expect("fold"), "iı");
    }

    #[test]
    fn case_folding_does_not_normalize_unicode() {
        let composed = unicode_default_casefold_v1("É").expect("fold");
        let decomposed = unicode_default_casefold_v1("E\u{301}").expect("fold");
        assert_ne!(composed, decomposed);
    }
}
