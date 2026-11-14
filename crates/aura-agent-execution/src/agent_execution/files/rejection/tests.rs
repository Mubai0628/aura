use super::*;
use aura_domain_common::WorkspacePath;
use std::path::PathBuf;

fn message(refusal: &Value) -> &str {
    refusal["message"].as_str().unwrap_or("")
}

/// A write to a file another program holds (a log Start-Process is writing) says why, as a
/// read of a file it holds without sharing already did.
#[test]
fn a_write_to_a_file_in_use_is_named_as_such() {
    let error = anyhow::Error::from(F::ChangesetFailed {
        failed_step: 0,
        applied_steps: 0,
        compensated_steps: 0,
        cause: r"failed to hold workspace object for the staged move \\?\C:\w\server.log: The process cannot access the file because it is being used by another process. (os error 32)".into(),
        journal_fault: String::new(),
    });
    let refusal = rejection("fs.write", &Value::Null, &error).expect("named");
    assert_eq!(refusal["code"], "fs.file_in_use");
    assert_eq!(refusal["retryable"], true);
    assert!(message(&refusal).contains("Nothing was changed."));
    let applied = anyhow::Error::from(F::ChangesetFailed {
        failed_step: 1,
        applied_steps: 1,
        compensated_steps: 0,
        cause: "(os error 32)".into(),
        journal_fault: String::new(),
    });
    assert!(rejection("fs.write", &Value::Null, &applied).is_none(), "a step had applied");
}

/// A target that changed between approval and start says so and that the call may be made
/// again, not the generic "check the arguments".
#[test]
fn a_target_changed_since_approval_is_named_and_retryable() {
    for code in ["agent.source_or_scope_changed", "agent.file_changed"] {
        let refusal = rejection("fs.write", &Value::Null, &anyhow::anyhow!(code)).expect("named");
        assert_eq!(refusal["code"], "fs.target_changed");
        assert_eq!(refusal["retryable"], true);
        assert!(message(&refusal).contains("Call it again"));
    }
}

/// A range over the read limit names the limit, so the next read fits it.
#[test]
fn a_range_over_the_read_limit_names_the_limit() {
    let error =
        anyhow::Error::from(F::TextRangeTooLarge { line_limit: 500, character_limit: 65_536 });
    let refusal = rejection("fs.read", &Value::Null, &error).expect("named");
    assert_eq!(refusal["code"], "fs.range_too_large");
    let message = message(&refusal);
    assert!(message.contains("at most 500 lines or 65,536 characters"), "{message}");
}

/// fs.read's description names the read limits Aura sets unless the user's tool settings
/// change them, written as a refusal names them.
#[test]
fn the_read_tools_description_names_the_default_read_limits() {
    let policy = aura_fs_tools::FsToolPolicy::default();
    let release = aura_tool_registry::agent_release::release().expect("release");
    let description = &release.entry("fs.read").expect("fs.read").definition.model_description;
    for limit in [
        format!("{} MiB", policy.full_text_read_limit_bytes / (1024 * 1024)),
        format!("{} lines", grouped(policy.small_range_line_limit)),
        format!("{} characters", grouped(policy.small_range_character_limit)),
    ] {
        assert!(description.contains(&limit), "{limit}: {description}");
    }
}

/// A call stopped before it started says why, what it left as it was and whether it can be
/// made again, instead of the generic "the execution did not succeed".
#[test]
fn a_call_stopped_before_it_started_says_why() {
    let withdrawn = anyhow::anyhow!("agent.permission_withdrawn");
    let refusal = rejection("fs.write", &Value::Null, &withdrawn).expect("named");
    assert_eq!(refusal["code"], "agent.permission_withdrawn");
    assert_eq!(refusal["category"], "permission");
    assert_eq!(refusal["retryable"], true);
    let text = message(&refusal);
    assert!(text.starts_with("This call was not started: the session's permission mode"), "{text}");
    assert!(text.contains("Nothing was changed."), "{text}");
    for code in ["agent.cancelled", "agent.not_running_batch", "agent.call_already_finished"] {
        let refusal = rejection("fs.read", &Value::Null, &anyhow::anyhow!(code)).expect("named");
        assert_eq!(refusal["category"], "cancelled", "{code}");
        assert!(message(&refusal).contains("was being stopped"), "{refusal}");
    }
    let ended = anyhow::anyhow!("agent.scope_refused: tool.scope.task_ended");
    let refusal = rejection("fs.read", &Value::Null, &ended).expect("named");
    assert_eq!(refusal["code"], "agent.scope_refused");
    assert!(message(&refusal).contains("had already ended"), "{refusal}");
    let missing_leaf = anyhow::anyhow!("agent.file_leaf_missing");
    let refusal = rejection("fs.write", &Value::Null, &missing_leaf).expect("named");
    assert!(message(&refusal).contains("Give the full path"), "{refusal}");
}

/// Every refused call that could have changed something says it did not: a write, the /init
/// write and a command save say nothing was changed, a delete that nothing was deleted. A read
/// changes nothing, so it does not say so.
#[test]
fn a_refusal_says_what_the_call_left_as_it_was() {
    let error = anyhow::Error::from(F::Workspace(W::Offline));
    for (action, effect) in [
        ("fs.write", Some("Nothing was changed.")),
        ("fs.patch", Some("Nothing was changed.")),
        ("workspace_init.complete", Some("Nothing was changed.")),
        ("commands.save", Some("Nothing was changed.")),
        ("fs.delete", Some("Nothing was deleted.")),
        ("fs.read", None),
    ] {
        let refusal = rejection(action, &Value::Null, &error).expect("named");
        assert_eq!(refusal["code"], "workspace.offline");
        let text = message(&refusal);
        match effect {
            Some(effect) => assert!(text.contains(effect), "{action}: {text}"),
            None => assert!(!text.contains("Nothing was"), "{action}: {text}"),
        }
        assert!(text.contains("reopen"), "{text}");
    }
}

/// fs.delete's own refusals say in their own words that nothing was deleted, so the English
/// sentence is not added; the refusal for a call with no object is one of them.
#[test]
fn a_delete_refusal_keeps_its_own_words() {
    let error = anyhow::anyhow!(
        "fs.delete.target_missing: 这次调用没有指向可删除的对象，没有删除任何东西。"
    );
    let refusal = rejection("fs.delete", &Value::Null, &error).expect("named");
    assert_eq!(refusal["code"], "fs.delete.target_missing");
    let text = message(&refusal);
    assert!(text.starts_with("这次调用没有指向可删除的对象"), "{text}");
    assert!(!text.contains("Nothing was deleted."), "{text}");
}

/// A missing file is named with its path, and each file tool gets a next step.
#[test]
fn a_missing_path_is_named_with_a_next_step() {
    let error = anyhow::Error::from(W::NotFound {
        path: WorkspacePath::parse("src/main.rs").expect("path"),
    });
    let read = rejection("fs.read", &Value::Null, &error).expect("named");
    assert_eq!(read["code"], "fs.not_found");
    assert_eq!(read["category"], "not_found");
    assert_eq!(read["retryable"], false);
    assert_eq!(message(&read), "src/main.rs does not exist. Use fs.find or fs.list to locate it.");
    let patch = rejection("fs.patch", &Value::Null, &error).expect("named");
    assert!(message(&patch).contains("Nothing was changed. Create it with fs.write"), "{patch}");
    // A read whose path was found missing when its facts were taken names that path.
    let facts = serde_json::json!({"paths": [{"path": "src/main.rs", "absent": true}]});
    let absent = not_found("fs.read", &facts);
    assert_eq!(absent["code"], "fs.not_found");
    assert_eq!(
        message(&absent),
        "src/main.rs does not exist. Use fs.find or fs.list to locate it."
    );
    let folder =
        not_found("fs.list", &serde_json::json!({"paths": [{"path": "docs", "absent": true}]}));
    assert!(message(&folder).starts_with("docs does not exist."), "{folder}");
    assert!(message(&folder).contains("Use fs.list on a parent folder"), "{folder}");
}

/// Windows' denial names the path as a person writes it, and what to do.
#[test]
fn a_denied_access_names_the_path() {
    let error = anyhow::Error::from(F::Io {
        action: "open",
        path: PathBuf::from(r"\\?\C:\w\locked.txt"),
        source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
    });
    let refusal = rejection("fs.read", &Value::Null, &error).expect("named");
    assert_eq!(refusal["code"], "fs.permission_denied");
    assert_eq!(refusal["category"], "permission");
    let text = message(&refusal);
    assert!(text.starts_with(r"Windows denied access to C:\w\locked.txt."), "{text}");
    assert!(text.contains("ask the user"), "{text}");
}

/// An operating-system error of a less common kind is named too, its own text quoted on one
/// line after Aura's sentence.
#[test]
fn an_uncommon_os_error_is_quoted_after_a_sentence() {
    let error = anyhow::Error::from(F::Io {
        action: "read",
        path: PathBuf::from(r"C:\w\a.txt"),
        source: std::io::Error::other("the device\nis not ready"),
    });
    let refusal = rejection("fs.read", &Value::Null, &error).expect("named");
    assert_eq!(refusal["code"], "fs.io_failed");
    assert_eq!(
        message(&refusal),
        r"Windows could not read C:\w\a.txt: the device is not ready. Check the path; if it is right, try again."
    );
}

/// The refusals of outside files that the file tools word themselves reach the model as
/// sentences with a next step, not as their codes.
#[test]
fn an_outside_file_refusal_reads_as_a_sentence() {
    let contents = anyhow::anyhow!(
        "agent.external_scope_contents: the directory is authorized as an object, but its \
         contents are outside this session's authorized scope; the grant covers C:\\data and \
         its direct children only"
    );
    let refusal = rejection("fs.list", &Value::Null, &contents).expect("named");
    assert_eq!(refusal["code"], "fs.scope_denied");
    assert!(message(&refusal).contains(r"granted access to C:\data and"), "{refusal}");
    let stale = anyhow::anyhow!(
        "agent.external_list_cursor_stale: the directory changed or the cursor belongs to \
         another listing; restart with cursor=null"
    );
    let refusal = rejection("fs.list", &Value::Null, &stale).expect("named");
    assert_eq!(refusal["code"], "fs.cursor_stale");
    assert!(message(&refusal).contains("cursor set to null"), "{refusal}");
    let held = anyhow::anyhow!(
        "agent.external_file_unavailable: The process cannot access the file because it is \
         being used by another process. (os error 32)"
    );
    let refusal = rejection("fs.read", &Value::Null, &held).expect("named");
    assert_eq!(refusal["code"], "fs.file_in_use");
    let replaced = anyhow::anyhow!(
        "agent.external_object_replaced: the authorized object is no longer the one at this \
         path; the operation was refused instead of acting on a different object"
    );
    let refusal = rejection("fs.read", &Value::Null, &replaced).expect("named");
    assert_eq!(refusal["code"], "fs.external_file_replaced");
    assert!(message(&refusal).contains("approve it again"), "{refusal}");
    assert!(!message(&refusal).contains("agent."), "{refusal}");
}

/// A long cause gives way, so what the call changed and what to do next are still read.
#[test]
fn a_long_cause_keeps_its_effect_and_next_step() {
    let error = anyhow::Error::from(F::InvalidPatch("x".repeat(2_000)));
    let refusal = rejection("fs.patch", &Value::Null, &error).expect("named");
    let text = message(&refusal);
    assert!(text.chars().count() <= MAX_MESSAGE_CHARS, "{}", text.chars().count());
    assert!(
        text.ends_with(
            "Nothing was changed. Read the file again and send a corrected diff, or write the \
             whole file with fs.write."
        ),
        "{text}"
    );
}

/// A workspace the call cannot use is named, and an index still being built asks to wait
/// without a count that would differ between preparation and start.
#[test]
fn workspace_states_are_named() {
    let archived = rejection("fs.read", &Value::Null, &anyhow::Error::from(W::Archived));
    assert_eq!(archived.expect("named")["code"], "workspace.archived");
    let building = anyhow::Error::from(W::IndexBuilding { listed: 1_234 });
    let refusal = rejection("fs.search", &Value::Null, &building).expect("named");
    assert_eq!(refusal["code"], "workspace.catalog_unavailable");
    assert_eq!(refusal["retryable"], true);
    assert!(!message(&refusal).contains("1234") && !message(&refusal).contains("1,234"));
    let stopped = rejection("fs.search", &Value::Null, &anyhow::Error::from(F::Cancelled));
    assert_eq!(stopped.expect("named")["category"], "cancelled");
}
