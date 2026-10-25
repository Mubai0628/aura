//! A Hook reads one JSON document from stdin and must see the end of its input.
use super::{super::*, support::*};
use std::{sync::atomic::AtomicBool, time::Duration};

#[test]
fn a_document_is_written_then_closed_so_the_program_reads_to_its_end() {
    let root = tempfile::tempdir().expect("temporary directory");
    // `sort` only answers after its input ends, so a still-open pipe would time out.
    let result = command(
        root.path(),
        "sort",
        &AtomicBool::new(false),
        Duration::from_secs(10),
        65536,
        ProgramInput::Document(b"zebra\r\napple\r\n"),
        &mut |_: &[u8]| Ok(vec![]),
    );
    assert_eq!(result.reason, ProgramStop::Exited, "{result:?}");
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stop_confirmed);
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).split_whitespace().collect::<Vec<_>>(),
        ["apple", "zebra"]
    );
}

#[test]
fn a_program_that_ignores_a_large_document_still_ends_normally() {
    let root = tempfile::tempdir().expect("temporary directory");
    // Larger than any pipe buffer: the writer is left with a closed pipe, which is the
    // program's choice and not a protocol failure.
    let document = vec![b'x'; 512 * 1024];
    let result = command(
        root.path(),
        "echo ignored-input",
        &AtomicBool::new(false),
        Duration::from_secs(10),
        65536,
        ProgramInput::Document(&document),
        &mut |_: &[u8]| Ok(vec![]),
    );
    assert_eq!(result.reason, ProgramStop::Exited, "{result:?}");
    assert_eq!(result.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&result.stdout).contains("ignored-input"));
}
