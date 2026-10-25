use super::*;

#[test]
fn mutation_receipts_include_remaining_changes_and_the_previous_branch_head() {
    let f = base();
    let before =
        String::from_utf8(f.git(&["rev-parse", "HEAD"])).expect("previous head").trim().to_owned();
    f.file("tracked", b"selected change\n");
    f.file("remaining.txt", b"untracked change\n");
    let mut w = writer(&f, Arc::new(AtomicBool::new(false)));
    let commit = serde_json::to_value(w.commit(&selected()).expect("commit")).expect("receipt");
    assert_eq!(commit["status"]["branch"], "main");
    assert_eq!(commit["status"]["head"], commit["commit"]["commit"]);
    assert_eq!(commit["status"]["total_entries"], 1);
    assert_eq!(commit["status"]["entries"][0]["path"], "remaining.txt");
    let branch = serde_json::to_value(
        w.branch(&GitBranchInput::Create {
            name: "receipt-branch".into(),
            start: Some(before.clone()),
            switch: true,
        })
        .expect("branch switch"),
    )
    .expect("receipt");
    assert_eq!(branch["previous_branch"], "main");
    assert_eq!(branch["previous_head"], commit["commit"]["commit"]);
    assert_eq!(branch["head"], before);
    assert_eq!(branch["status"]["head"], before);
    assert_eq!(branch["status"]["branch"], "receipt-branch");
    assert_eq!(branch["status"]["total_entries"], 1);
}

#[test]
fn an_unborn_branch_can_be_created_switched_and_receive_its_first_commit() {
    let f = Fixture::new("");
    let mut w = writer(&f, Arc::new(AtomicBool::new(false)));
    let branch = w
        .branch(&GitBranchInput::Create { name: "first-branch".into(), start: None, switch: true })
        .expect("unborn branch");
    assert!(branch.head.is_none() && branch.created && branch.switched);
    f.file("tracked", b"first commit\n");
    let commit = w.commit(&selected()).expect("root commit");
    assert!(commit.commit.parents.is_empty());
    assert_eq!(commit.branch.as_deref(), Some("first-branch"));
    assert_eq!(f.git(&["show", "HEAD:tracked"]), b"first commit\n");
}

#[test]
fn creating_at_an_explicit_start_does_not_switch_the_current_branch() {
    let f = base();
    let start = String::from_utf8(f.git(&["rev-parse", "HEAD"])).expect("start").trim().to_owned();
    f.file("tracked", b"later\n");
    f.commit("later");
    let out = writer(&f, Arc::new(AtomicBool::new(false)))
        .branch(&GitBranchInput::Create {
            name: "at-base".into(),
            start: Some(start.clone()),
            switch: false,
        })
        .expect("create only");
    assert_eq!(out.head.as_deref(), Some(start.as_str()));
    assert!(out.created && !out.switched);
    assert_eq!(f.git(&["symbolic-ref", "--short", "HEAD"]), b"main\n");
}

#[test]
fn signing_is_not_disabled_and_a_temporary_signer_failure_is_explained() {
    let f = base();
    f.file("tracked", b"signed change\n");
    f.file(
        ".git/fixture-signer",
        b"#!/bin/sh\nprintf called >signer.marker\necho 'fixture signing refused' >&2\nexit 1\n",
    );
    let signer = f.root.path().join(".git/fixture-signer").to_string_lossy().replace('\\', "/");
    f.git(&["config", "gpg.program", &signer]);
    f.git(&["config", "commit.gpgsign", "true"]);
    let before = f.git(&["rev-parse", "HEAD"]);
    let error = writer(&f, Arc::new(AtomicBool::new(false)))
        .commit(&selected())
        .expect_err("signer refuses")
        .to_string();
    assert!(f.root.path().join("signer.marker").is_file(), "Native Git ran the configured signer");
    assert!(
        error.contains("git.signing_failed") && error.contains("fixture signing refused"),
        "{error}"
    );
    assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
}
