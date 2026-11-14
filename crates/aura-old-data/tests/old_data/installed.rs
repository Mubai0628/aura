//! Which namespaces the installations in Settings > Apps use, from their `active.json`.
#![allow(clippy::unwrap_used)]
use crate::common;

use std::{io, path::PathBuf};

use aura_old_data::Installed;
use common::{namespace, production, write};

fn installation(folder: &std::path::Path, namespace: &str, extra: &str) -> PathBuf {
    write(
        &folder.join("active.json"),
        format!(
            r#"{{"active":"0.3.0","release_descriptor":{{"bundle_namespace":"{namespace}","identity":{{}}}}{extra}}}"#
        )
        .as_bytes(),
    );
    folder.to_path_buf()
}

#[test]
fn each_installation_names_its_namespace_even_with_fields_this_version_does_not_know() {
    let home = tempfile::tempdir().unwrap();
    let released = installation(&home.path().join("Aura"), "aura.desktop.production", "");
    let test = installation(
        &home.path().join("Aura Test run"),
        "aura.desktop.test.run",
        r#","written_by_0_4":{"x":1}"#,
    );
    assert_eq!(
        Installed::from_entries(Ok(vec![Some(released), Some(test)])),
        Installed::Known(vec![
            production().object_name_hash(),
            namespace("aura.desktop.test.run").object_name_hash()
        ])
    );
    assert_eq!(Installed::from_entries(Ok(Vec::new())), Installed::Known(Vec::new()));
}

#[test]
fn an_entry_that_cannot_be_read_makes_every_namespace_unknown() {
    let home = tempfile::tempdir().unwrap();
    let readable = installation(&home.path().join("Aura"), "aura.desktop.production", "");
    let moved_by_hand = home.path().join("Gone");
    let broken = home.path().join("Broken");
    write(&broken.join("active.json"), b"{\"active\":");
    for entries in [
        vec![Some(readable.clone()), None],
        vec![Some(readable.clone()), Some(moved_by_hand)],
        vec![Some(broken), Some(readable)],
    ] {
        assert!(
            matches!(Installed::from_entries(Ok(entries.clone())), Installed::Unknown(_)),
            "{entries:?}"
        );
    }
    assert!(matches!(
        Installed::from_entries(Err(io::Error::from(io::ErrorKind::PermissionDenied))),
        Installed::Unknown(_)
    ));
}
