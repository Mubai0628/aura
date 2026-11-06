//! A capture the composer sends: what a terminal printed or a page showed, with the name it is
//! kept under. Checked like a text file, and compared by its digest when a command is repeated.
use aura_agent_domain::{
    AgentAttachmentDraft, AttachmentError, MAX_CAPTURED_TEXT_BYTES, validate_attachment_drafts,
};

fn capture(name: &str, text: &str) -> AgentAttachmentDraft {
    AgentAttachmentDraft::Capture { name: name.into(), text: text.into() }
}

#[test]
fn a_capture_is_checked_for_its_name_and_size_and_counts_toward_the_message() {
    assert_eq!(validate_attachment_drafts(&[capture("终端 1.txt", "PS> dir\n")]), Ok(()));
    assert_eq!(
        validate_attachment_drafts(&[capture("..\\终端.txt", "x")]),
        Err(AttachmentError::NameInvalid)
    );
    let over = "x".repeat(usize::try_from(MAX_CAPTURED_TEXT_BYTES).expect("small") + 1);
    assert!(matches!(
        validate_attachment_drafts(&[capture("page.txt", &over)]),
        Err(AttachmentError::CaptureTooLarge(name)) if name == "page.txt"
    ));
    // Five captures at the limit go past what one message carries.
    let full = "x".repeat(usize::try_from(MAX_CAPTURED_TEXT_BYTES).expect("small"));
    let five: Vec<_> = (0..5).map(|i| capture(&format!("{i}.txt"), &full)).collect();
    assert_eq!(validate_attachment_drafts(&five), Err(AttachmentError::TooLarge));
}

#[test]
fn a_captures_receipt_keeps_its_name_and_a_digest_of_its_text() {
    let draft = capture("终端 1.txt", "PS> dir\n");
    let AgentAttachmentDraft::Capture { name, text } = draft.receipt() else {
        panic!("a capture stays a capture");
    };
    assert_eq!(name, "终端 1.txt");
    assert!(text.starts_with("sha256:"), "{text}");
    assert_eq!(draft.receipt(), draft.receipt());
    let json = serde_json::to_value(&draft).expect("json");
    assert_eq!(json["kind"], "capture");
}
