//! Which stores need a copy before this version's first start: each store's own check, with the
//! version it replaces. An answer that is an error copies that store.
use std::{io::Write, path::Path};

use serde_json::{Value, json};

fn answer_of(pending: Result<bool, String>) -> Value {
    match pending {
        Ok(pending) => json!(pending),
        Err(error) => json!({ "error": error }),
    }
}

pub(crate) fn answer(root: &Path, replacing: Option<&str>, out: &mut dyn Write) -> u8 {
    let legacy = aura_persistence::copy_before_start(&root.join(crate::LEGACY), replacing)
        .map_err(|error| error.to_string());
    let execution = aura_persistence_v2::copy_before_start(
        &root.join(crate::EXECUTION).join("aura-v2.db"),
        replacing,
    )
    .map_err(|error| error.to_string());
    let answer = json!({
        "data/aura.db": answer_of(legacy),
        "data/v2-execution/aura-v2.db": answer_of(execution),
    });
    let _ = writeln!(out, "{answer}");
    0
}
