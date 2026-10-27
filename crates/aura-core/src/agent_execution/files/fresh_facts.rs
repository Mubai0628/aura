//! The facts a file call recorded when it was prepared, taken again just before it starts. A read
//! reads whatever the file holds when it runs, so its content changing in between (a log a running
//! program appends to) does not stop it. A new file whose nearest existing folder moved deeper along
//! its own path (another write of the same response created the folder) is still the same new file
//! when only ordinary folders appeared. Anything else that differs stops the call.
use super::*;
use serde_json::Value;

/// Whether the facts taken now still allow the call that was prepared with `prepared`.
pub(super) fn still_hold(
    service: &AgentExecution,
    call: &CallContext,
    prepared: &Value,
    fresh: &Value,
) -> anyhow::Result<bool> {
    if prepared == fresh {
        return Ok(true);
    }
    if prepared["root_identity"] != fresh["root_identity"] {
        return Ok(false);
    }
    let (Some(before), Some(now)) = (prepared["paths"].as_array(), fresh["paths"].as_array())
    else {
        return Ok(false);
    };
    if before.len() != now.len() {
        return Ok(false);
    }
    let reads = matches!(
        call.operation.content.action_kind.as_str(),
        "fs.read" | "fs.list" | "fs.find" | "fs.search"
    );
    for (was, is) in before.iter().zip(now) {
        if was == is {
            continue;
        }
        if was["path"] != is["path"] {
            return Ok(false);
        }
        if reads && same_object(was, is) {
            continue;
        }
        if was["absent"] == true && is["absent"] == true && deeper(&was["parent"], &is["parent"]) {
            let path: WorkspacePath = serde_json::from_value(is["path"].clone())?;
            if service.only_ordinary_folders_to(call.snapshot.workspace_id, &path)? {
                continue;
            }
        }
        return Ok(false);
    }
    Ok(true)
}

/// The same file or folder, whatever it holds now.
fn same_object(was: &Value, is: &Value) -> bool {
    was["absent"].is_null()
        && is["absent"].is_null()
        && !was["identity"].is_null()
        && was["identity"] == is["identity"]
        && was["kind"] == is["kind"]
}

/// Whether the folder `now` is `was` or below it.
fn deeper(was: &Value, now: &Value) -> bool {
    let (Some(was), Some(now)) = (was.as_str(), now.as_str()) else { return false };
    if was == now {
        return true;
    }
    if was == WorkspacePath::ROOT {
        return now != WorkspacePath::ROOT;
    }
    now.starts_with(&format!("{was}/"))
}

#[cfg(test)]
mod tests {
    use super::{deeper, same_object};
    use serde_json::json;

    #[test]
    fn a_file_that_only_changed_its_content_is_the_same_object() {
        let identity = json!({"volume_serial_number":"1","file_id":"7"});
        let was = json!({"path":"server.log","identity":identity,"kind":"file","hash":"a"});
        let is = json!({"path":"server.log","identity":identity,"kind":"file","hash":"b"});
        assert!(same_object(&was, &is));
        let replaced = json!({"path":"server.log","identity":{"volume_serial_number":"1","file_id":"8"},"kind":"file"});
        assert!(!same_object(&was, &replaced), "another file at the same path is not");
        let absent = json!({"path":"server.log","absent":true});
        assert!(!same_object(&absent, &absent));
    }

    #[test]
    fn a_nearest_folder_may_only_move_deeper_along_the_path() {
        assert!(deeper(&json!("."), &json!("game")));
        assert!(deeper(&json!("game"), &json!("game/server")));
        assert!(deeper(&json!("game"), &json!("game")));
        assert!(!deeper(&json!("game"), &json!("gameplay")));
        assert!(!deeper(&json!("game/server"), &json!("game")));
        assert!(!deeper(&json!("src"), &json!("game/src")));
    }
}
