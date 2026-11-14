//! `--store-check <data folder>`: SQLite's quick check of each store in a data folder, as a move
//! checks its copy before the data is trusted there. One line, `{"data/aura.db": …,
//! "data/v2-execution/aura-v2.db": …}`, each `true` when it passed, `null` when the store is not
//! there and `{"error": …}` when it failed; exit 0 when none failed, 1 otherwise.
use std::{io::Write, path::Path};

use serde_json::{Map, Value, json};

const STORES: [&str; 2] = [crate::LEGACY, "data/v2-execution/aura-v2.db"];

/// Each store's check: `None` when the store is not there.
fn checked(root: &Path) -> Vec<(&'static str, Option<Result<(), String>>)> {
    STORES
        .iter()
        .map(|store| {
            let path = root.join(store);
            let result = path.is_file().then(|| {
                aura_persistence_core::schema::quick_check(&path).map_err(|error| error.to_string())
            });
            (*store, result)
        })
        .collect()
}

/// The quick check of every store under `root` that is there; the failures, named by store.
pub fn check_stores(root: &Path) -> Result<(), String> {
    let failed: Vec<String> = checked(root)
        .into_iter()
        .filter_map(|(store, result)| match result {
            Some(Err(error)) => Some(format!("{store}: {error}")),
            _ => None,
        })
        .collect();
    if failed.is_empty() { Ok(()) } else { Err(failed.join("; ")) }
}

pub(crate) fn run(root: &Path, out: &mut dyn Write) -> u8 {
    let mut failed = false;
    let mut answer = Map::new();
    for (store, result) in checked(root) {
        let value = match result {
            None => Value::Null,
            Some(Ok(())) => json!(true),
            Some(Err(error)) => {
                failed = true;
                json!({ "error": error })
            }
        };
        answer.insert(store.to_owned(), value);
    }
    let _ = writeln!(out, "{}", Value::Object(answer));
    u8::from(failed)
}
