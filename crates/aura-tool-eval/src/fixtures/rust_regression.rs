use anyhow::{Result, ensure};
use quote::ToTokens;
use std::collections::BTreeMap;
use std::path::Path;
use syn::Item;

/// Reintroduce original function bodies, retaining model tests and additional helper items.
pub(super) fn restore(original: &Path, candidate: &Path) -> Result<()> {
    let original = syn::parse_file(&std::fs::read_to_string(original)?)?;
    let mut candidate_file = syn::parse_file(&std::fs::read_to_string(candidate)?)?;
    let functions = original
        .items
        .into_iter()
        .filter_map(|item| match item {
            Item::Fn(function) => Some(function),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure!(!functions.is_empty(), "eval.fixture_function_missing");
    for function in functions {
        let target = candidate_file
            .items
            .iter_mut()
            .find_map(|item| match item {
                Item::Fn(candidate) if candidate.sig.ident == function.sig.ident => Some(candidate),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("eval.candidate_function_missing"))?;
        target.block = function.block;
    }
    std::fs::write(candidate, candidate_file.into_token_stream().to_string())?;
    Ok(())
}

pub(super) fn has_test_edit(original: &Path, candidate: &Path) -> Result<bool> {
    let before = if original.exists() { tests(original)? } else { BTreeMap::new() };
    Ok(tests(candidate)?.iter().any(|(name, body)| before.get(name) != Some(body)))
}

fn tests(path: &Path) -> Result<BTreeMap<String, String>> {
    let file = syn::parse_file(&std::fs::read_to_string(path)?)?;
    let mut found = BTreeMap::new();
    collect(&file.items, "", &mut found);
    Ok(found)
}

fn collect(items: &[Item], prefix: &str, found: &mut BTreeMap<String, String>) {
    for item in items {
        match item {
            Item::Fn(function)
                if function.attrs.iter().any(|attr| attr.path().is_ident("test")) =>
            {
                found.insert(
                    format!("{prefix}{}", function.sig.ident),
                    function.to_token_stream().to_string(),
                );
            }
            Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    collect(nested, &format!("{prefix}{}::", module.ident), found);
                }
            }
            _ => {}
        }
    }
}
