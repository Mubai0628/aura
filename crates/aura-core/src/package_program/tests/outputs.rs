use crate::package_program::outputs::collect;
use aura_domain_common::ContentHash;
use aura_tool_contract::{
    StdioArtifactReadyPayloadV1, ToolOutputRelativePathV1, ToolOutputSlotKeyV1,
};
use std::fs;

fn declaration(path: &str) -> StdioArtifactReadyPayloadV1 {
    StdioArtifactReadyPayloadV1 {
        slot: ToolOutputSlotKeyV1::new("one").expect("slot"),
        relative_path: ToolOutputRelativePathV1::new(path).expect("path"),
        media_type: "text/plain; charset=utf-8".into(),
        byte_length: 4,
        content_hash: ContentHash::sha256(b"safe"),
    }
}
#[test]
fn outputs_require_declared_access_and_verified_length_hash_identity_and_media() {
    let root = tempfile::tempdir().expect("temporary output");
    fs::write(root.path().join("out.txt"), b"safe").expect("file");
    let good = declaration("out.txt");
    assert!(collect(root.path(), false, std::slice::from_ref(&good)).is_err());
    assert_eq!(
        collect(root.path(), true, &[good.clone(), good.clone()])
            .expect("idempotent same slot")
            .len(),
        1
    );
    for field in 0..3 {
        let mut bad = good.clone();
        match field {
            0 => bad.byte_length = 3,
            1 => bad.content_hash = ContentHash::sha256(b"evil"),
            _ => bad.media_type = "text/plain\r\nInjected: yes".into(),
        };
        assert!(collect(root.path(), true, &[bad]).is_err());
    }
    fs::hard_link(root.path().join("out.txt"), root.path().join("alias.txt"))
        .expect("only temporary hard link");
    assert!(collect(root.path(), true, &[declaration("alias.txt")]).is_err());
}
#[test]
fn output_junction_cannot_escape_the_call_root_or_delete_the_outside_fixture() {
    let root = tempfile::tempdir().expect("temporary output");
    let outside = tempfile::tempdir().expect("temporary outside");
    fs::write(outside.path().join("out.txt"), b"safe").expect("outside fixture");
    junction::create(outside.path(), root.path().join("escape")).expect("temporary junction");
    assert!(collect(root.path(), true, &[declaration("escape/out.txt")]).is_err());
    drop(root);
    assert_eq!(
        fs::read(outside.path().join("out.txt")).expect("outside survives cleanup"),
        b"safe"
    );
}
