use aura_domain_common::FileIdentity;
use aura_security_contract::{
    FileConsentProtection, FileConsentTarget, ResolvedCapability as C, file_consent_capabilities,
    file_consent_target_covers, file_entry_key, file_object_key,
};

fn object(id: &str, path: &str) -> FileConsentTarget {
    FileConsentTarget {
        canonical_path: path.into(),
        identity_key: file_object_key(&FileIdentity {
            volume_serial_number: "volume".into(),
            file_id: id.into(),
        }),
        ..Default::default()
    }
}

#[test]
fn s06_d5_case_distinct_objects_never_merge_and_aliases_keep_the_same_object() {
    let approved = object("one", "C:\\sensitive\\A.txt");
    let candidate = object("two", "C:\\sensitive\\a.txt");
    assert!(!file_consent_target_covers(&approved, &candidate));
    let alias = object("one", "\\\\?\\C:\\sensitive\\A.txt");
    assert!(
        file_consent_target_covers(&approved, &alias),
        "both spellings name the same verified object"
    );
}

#[test]
fn s06_d5_native_entry_keys_do_not_merge_unicode_scalars_or_sensitive_names() {
    let parent =
        FileIdentity { volume_serial_number: "volume".into(), file_id: "directory".into() };
    for (first, second) in [("A.txt", "a.txt"), ("\u{10400}.txt", "\u{10428}.txt")] {
        let a = FileConsentTarget {
            identity_key: file_entry_key(&parent, first),
            ..Default::default()
        };
        let b = FileConsentTarget {
            identity_key: file_entry_key(&parent, second),
            ..Default::default()
        };
        assert!(!file_consent_target_covers(&a, &b), "sensitive native keys differ");
        assert!(file_consent_target_covers(&a, &a), "native bound entry remains approvable");
    }
}

#[test]
fn s06_b3_d1_recursive_scope_uses_verified_ancestry_and_carves_protected_targets() {
    let mut root = object("directory", "C:\\project");
    root.recursive = true;
    let mut deep = object("child", "C:\\project\\future\\deep\\note.txt");
    deep.ancestors = vec![root.identity_key.clone()];
    assert!(file_consent_target_covers(&root, &deep));
    let sibling = object("other", "C:\\project-sibling\\note.txt");
    assert!(!file_consent_target_covers(&root, &sibling));
    let linked_outside = object("outside", "C:\\project\\junction\\note.txt");
    assert!(!file_consent_target_covers(&root, &linked_outside));
    deep.protection = FileConsentProtection::System;
    assert!(!file_consent_target_covers(&root, &deep));
    root.protection = FileConsentProtection::System;
    assert!(
        file_consent_target_covers(&root, &deep),
        "explicit protected-root approval covers its own descendants"
    );
    assert!(
        !file_consent_target_covers(&deep, &root),
        "child approval cannot authorize its parent"
    );
}

#[test]
fn s06_unknown_target_facts_fail_closed_even_when_display_strings_match() {
    let unknown = FileConsentTarget { canonical_path: "C:\\same.txt".into(), ..Default::default() };
    assert!(!file_consent_target_covers(&unknown, &unknown));
}

#[test]
fn s06_d1_protected_parent_does_not_inherit_another_protection_class() {
    use aura_security_contract::{CanonicalPath, ResourceDescriptor, file_consent_target_of};
    let mut root = object("system-root", "C:\\protected");
    root.recursive = true;
    root.protection = FileConsentProtection::System;
    let mut child = object("credential-store", "C:\\protected\\credentials");
    child.protection = FileConsentProtection::SensitiveUser;
    child.ancestors = vec![root.identity_key.clone()];
    let path = |display: &str| CanonicalPath {
        display: display.into(),
        utf16: display.encode_utf16().collect(),
    };
    let approved = file_consent_target_of(&ResourceDescriptor::SystemPath {
        canonical_path: path(&root.canonical_path),
        identity: None,
        parent_identity: None,
        file_target: Some(root),
    })
    .expect("the system resource exposes its verified file consent target");
    let candidate = file_consent_target_of(&ResourceDescriptor::SensitiveUserPath {
        canonical_path: path(&child.canonical_path),
        identity: None,
        parent_identity: None,
        file_target: Some(child),
    })
    .expect("the sensitive user resource exposes its verified file consent target");
    assert!(
        !file_consent_target_covers(&approved, &candidate),
        "a different protected range needs its own explicit approval"
    );
}

#[test]
fn s06_c1_a4_write_scope_includes_ordinary_mutations_without_network_escalation() {
    let scope = file_consent_capabilities(&[C::WorkspaceFileWrite]);
    for c in [
        C::WorkspaceFileCreate,
        C::WorkspaceFileWrite,
        C::WorkspaceFileDelete,
        C::WorkspaceFileMove,
    ] {
        assert!(scope.contains(&c));
    }
    assert!(!scope.contains(&C::DataEgress));
    assert_eq!(file_consent_capabilities(&[C::ExternalFileRead]), vec![C::ExternalFileRead]);
    assert!(file_consent_capabilities(&[C::ExternalFileWrite]).contains(&C::ExternalFileDelete));
}
