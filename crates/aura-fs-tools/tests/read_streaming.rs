//! Fix 1 (`fs.write` encoding preservation) and Fix 2 (bounded-memory range
//! reads for large files) verified through the real `FsToolService` over a real
//! workspace, with the whole-file path as the oracle.
use std::{path::Path, sync::Arc, time::Duration};

use aura_domain_common::{
    ContentHash, CorrelationId, Revision, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath,
};
use aura_fs_tools::{FsToolError, FsToolPolicy, FsToolService};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{
    CatalogOptions, DiskTextSnapshot, LOGICAL_TEXT_STREAM_CHUNK_BYTES, WorkspaceRuntime,
    WorkspaceRuntimeOptions,
};
use aura_workspace_domain::{
    BufferEditCommand, BufferOpenCommand, FileStorageSource, FsExpectedState, FsReadTextRequest,
    FsReadTextResult, FsWriteFileRequest, LargeFileReadMode, LineEnding, SessionSummary,
    TextEncoding, TextReadRange, WorkspaceCreateCommand, WorkspaceFileTarget,
};

const IN_MEMORY_LIMIT: u64 = 8 * 1024 * 1024;
const STREAMING_LIMIT: u64 = 4 * 1024;
const CHUNK: usize = LOGICAL_TEXT_STREAM_CHUNK_BYTES;

struct Harness {
    workspace: Arc<WorkspaceRuntime>,
    persistence: Arc<PersistenceCoordinator>,
    root: tempfile::TempDir,
    _data: tempfile::TempDir,
    workspace_id: WorkspaceId,
    session_id: SessionId,
}

impl Harness {
    fn new(setup: impl FnOnce(&Path)) -> Self {
        let root = tempfile::tempdir().expect("workspace root");
        setup(root.path());
        let data = tempfile::tempdir().expect("data root");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            data.path().join("aura.sqlite3"),
            data.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence startup")
        {
            PersistenceStartup::Ready(value) => Arc::new(value),
            PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected recovery: {}", value.reason)
            }
        };
        let workspace = WorkspaceRuntime::open(
            Arc::clone(&persistence),
            WorkspaceRuntimeOptions {
                catalog: CatalogOptions::with_index_file_limit(64 * 1024 * 1024),
                watcher_coalesce: Duration::from_millis(20),
                watcher_poll_interval: Duration::from_secs(1),
                watcher_rescan_attempts: 1,
                ..WorkspaceRuntimeOptions::default()
            },
        )
        .expect("workspace runtime");
        let created = workspace
            .create(
                WorkspaceCreateCommand {
                    selected_root: root.path().display().to_string(),
                    display_name: "read streaming".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create workspace");
        workspace.wait_until_indexed(std::time::Duration::from_secs(60)).expect("the first walk");
        let workspace_id = created.workspace.workspace_id;
        let session_id = SessionId::new();
        let current = persistence.workspace(workspace_id).expect("current workspace");
        persistence
            .upsert_workspace_session(
                current,
                SessionSummary {
                    session_id,
                    workspace_id,
                    title: "Streaming session".to_owned(),
                    state: "idle".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("create session");
        Self { workspace, persistence, root, _data: data, workspace_id, session_id }
    }

    /// A service whose whole-read limit decides whether `read_text` streams.
    fn service(
        &self,
        full_text_read_limit_bytes: u64,
        mode: LargeFileReadMode,
    ) -> Arc<FsToolService> {
        FsToolService::new(
            Arc::clone(&self.workspace),
            Arc::clone(&self.persistence),
            FsToolPolicy {
                large_file_read_mode: mode,
                full_text_read_limit_bytes,
                small_range_line_limit: 4_000,
                small_range_character_limit: 256,
                ..FsToolPolicy::default()
            },
        )
        .expect("fs service")
    }

    /// A request against one workspace path in this harness's session.
    fn request(&self, path: &str, range: TextReadRange) -> FsReadTextRequest {
        FsReadTextRequest {
            workspace_id: self.workspace_id,
            session_id: self.session_id,
            target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
            range,
            explicit_encoding: None,
        }
    }

    fn root_path(&self, path: &str) -> std::path::PathBuf {
        self.root.path().join(path)
    }
}

/// Every field `read_text` reports for the same input, so the in-memory path and
/// the streaming path can be compared field by field.
fn facts(result: &FsReadTextResult) -> (String, ContentHash, u64, u64, TextEncoding, LineEnding) {
    (
        result.text.clone(),
        result.logical_hash.clone(),
        result.total_characters,
        result.total_lines,
        result.encoding,
        result.line_ending,
    )
}

/// The whole-file oracle for one request.
fn whole_file(harness: &Harness, request: FsReadTextRequest) -> FsReadTextResult {
    harness
        .service(IN_MEMORY_LIMIT, LargeFileReadMode::RequireRange)
        .read_text(request)
        .expect("the whole-file path must succeed")
}

/// The streaming path for the same request, which must agree with it exactly.
fn streamed(harness: &Harness, request: FsReadTextRequest) -> FsReadTextResult {
    harness
        .service(STREAMING_LIMIT, LargeFileReadMode::RequireRange)
        .read_text(request)
        .expect("the streaming path must succeed")
}

/// Asserts the streaming path reports the same bytes, hash, counts, encoding and
/// line-ending convention as materializing the whole file.
fn assert_streaming_matches_whole_file(harness: &Harness, request: FsReadTextRequest) {
    let expected = whole_file(harness, request.clone());
    let actual = streamed(harness, request.clone());
    assert_eq!(
        facts(&actual),
        facts(&expected),
        "path={:?} range={:?} encoding={:?}",
        request.target.path.as_str(),
        request.range,
        request.explicit_encoding
    );
}

/// A range read of one file with an explicit encoding choice, compared against
/// the whole-file path for the same choice.
fn assert_explicit_encoding_matches(
    harness: &Harness,
    file: &str,
    range: TextReadRange,
    encoding: TextEncoding,
) {
    let mut request = harness.request(file, range);
    request.explicit_encoding = Some(encoding);
    assert_streaming_matches_whole_file(harness, request);
}

fn utf16le(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// A body with CRLF, bare LF, a bare CR, multi-byte characters, and enough lines
/// that both line and character ranges are meaningful.
fn mixed_body(lines: usize) -> String {
    let mut text = String::new();
    for index in 0..lines {
        match index % 4 {
            0 => text.push_str(&format!("ascii line {index}\r\n")),
            1 => text.push_str(&format!("中文行 {index} with é and 😀\n")),
            2 => text.push_str(&format!("cr only line {index}\r")),
            _ => text.push_str(&format!("emoji 😀 tail {index}\n")),
        }
    }
    text
}

fn large_body(bytes: usize) -> String {
    let mut text = String::with_capacity(bytes + 64);
    let mut index = 0_usize;
    while text.len() < bytes {
        text.push_str(&format!("line {index:08} payload abcdefghijklmnopqrstuvwxyz\n"));
        index += 1;
    }
    text
}

#[test]
fn streaming_range_read_matches_the_whole_file_path_field_by_field() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("mixed.txt"), mixed_body(900)).expect("mixed file");
    });
    let streaming = harness.service(STREAMING_LIMIT, LargeFileReadMode::RequireRange);
    let in_memory = harness.service(IN_MEMORY_LIMIT, LargeFileReadMode::RequireRange);

    let ranges = [
        TextReadRange::Lines { start_line_index: 0, count: 25 },
        TextReadRange::Lines { start_line_index: 300, count: 40 },
        TextReadRange::Lines { start_line_index: 0, count: 1 },
        TextReadRange::Lines { start_line_index: 899, count: 5 },
        TextReadRange::Characters { start_character_offset: 0, count: 200 },
        TextReadRange::Characters { start_character_offset: 5_000, count: 128 },
        TextReadRange::Characters { start_character_offset: 0, count: 0 },
    ];
    for range in ranges {
        let streamed = streaming
            .read_text(harness.request("mixed.txt", range.clone()))
            .expect("streamed read");
        let materialized = in_memory
            .read_text(harness.request("mixed.txt", range.clone()))
            .expect("in-memory read");
        assert_eq!(
            facts(&streamed),
            facts(&materialized),
            "streamed and whole-file reads disagree for {range:?}"
        );
        assert_eq!(streamed.source, FileStorageSource::Disk);
        assert_eq!(streamed.buffer_revision, None);
        assert!(!streamed.is_dirty);
        assert_eq!(streamed.returned_range, range);
    }

    // The exact whole-file hash and the exact whole-file counts survive streaming.
    let whole = harness
        .service(IN_MEMORY_LIMIT, LargeFileReadMode::AllowFull)
        .read_text(harness.request("mixed.txt", TextReadRange::Full))
        .expect("whole-file read");
    let ranged = streaming
        .read_text(
            harness.request("mixed.txt", TextReadRange::Lines { start_line_index: 10, count: 3 }),
        )
        .expect("ranged read");
    assert_eq!(ranged.logical_hash, whole.logical_hash);
    assert_eq!(ranged.disk_hash, whole.disk_hash);
    assert_eq!(ranged.total_characters, whole.total_characters);
    assert_eq!(ranged.total_lines, whole.total_lines);
    assert_eq!(ranged.encoding, whole.encoding);
    assert_eq!(ranged.line_ending, whole.line_ending);
}

#[test]
fn streaming_decodes_bom_utf16_and_characters_straddling_chunk_boundaries() {
    let mut expected_utf8 = String::new();
    let mut expected_utf16 = String::new();
    for index in 0..40_000_usize {
        expected_utf8.push_str(&format!("row {index:06} 中文 é 😀 tail\n"));
        expected_utf16.push_str(&format!("row {index:06} 中文 é 😀 tail\r\n"));
    }
    // Both files are far larger than the streaming threshold, so every read below
    // is streamed, and the boundary-straddling characters below are guaranteed to
    // sit across a chunk seam.
    let utf8_bom = {
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice(expected_utf8.as_bytes());
        bytes
    };
    let utf16 = utf16le(&expected_utf16);
    // A four-byte UTF-8 character starting one byte before the first chunk seam
    // and a three-byte character starting two bytes before the second one.
    let straddling_utf8 = {
        let mut text = String::new();
        text.push_str(&"a".repeat(CHUNK - 1));
        text.push('😀');
        text.push_str(&"b".repeat(CHUNK - 1 - "😀".len() - 2));
        text.push('中');
        text.push_str(&"c".repeat(CHUNK));
        text.push('\n');
        text
    };
    // A UTF-16 surrogate pair whose halves land in different chunks.
    let straddling_utf16 = {
        let mut text = String::new();
        text.push_str(&"d".repeat((CHUNK - 2) / 2));
        text.push('😀');
        text.push_str(&"e".repeat(CHUNK / 2));
        text.push('\n');
        text
    };
    let harness = Harness::new(|root| {
        std::fs::write(root.join("bom.txt"), &utf8_bom).expect("utf-8 bom file");
        std::fs::write(root.join("utf16.txt"), &utf16).expect("utf-16 file");
        std::fs::write(root.join("straddle-utf8.txt"), straddling_utf8.as_bytes())
            .expect("straddling utf-8 file");
        std::fs::write(root.join("straddle-utf16.txt"), utf16le(&straddling_utf16))
            .expect("straddling utf-16 file");
    });
    let streaming = harness.service(STREAMING_LIMIT, LargeFileReadMode::AllowFull);
    let in_memory = harness.service(IN_MEMORY_LIMIT, LargeFileReadMode::AllowFull);
    for (path, encoding) in [
        ("bom.txt", TextEncoding::Utf8Bom),
        ("utf16.txt", TextEncoding::Utf16Le),
        ("straddle-utf8.txt", TextEncoding::Utf8),
        ("straddle-utf16.txt", TextEncoding::Utf16Le),
    ] {
        let streamed = streaming
            .read_text(harness.request(path, TextReadRange::Full))
            .expect("streamed whole read");
        assert_eq!(streamed.encoding, encoding, "{path} encoding");
        let materialized = in_memory
            .read_text(harness.request(path, TextReadRange::Full))
            .expect("in-memory whole read");
        assert_eq!(facts(&streamed), facts(&materialized), "{path} whole-file equality");
        assert_eq!(streamed.line_ending, materialized.line_ending, "{path} line ending");
    }
    // The straddling characters decode as exactly one character each.
    let streamed = streaming
        .read_text(harness.request("straddle-utf8.txt", TextReadRange::Full))
        .expect("streamed straddling read");
    assert_eq!(streamed.text, straddling_utf8);
    assert_eq!(streamed.text.chars().filter(|value| *value == '😀').count(), 1);
    assert_eq!(streamed.text.chars().filter(|value| *value == '中').count(), 1);
    let streamed = streaming
        .read_text(harness.request("straddle-utf16.txt", TextReadRange::Full))
        .expect("streamed straddling utf-16 read");
    assert_eq!(streamed.text, straddling_utf16);
    assert_eq!(streamed.text.chars().filter(|value| *value == '😀').count(), 1);
}

#[test]
fn streaming_line_range_reads_return_the_same_lines_as_the_whole_file_path() {
    let body = large_body(1024 * 1024);
    let expected_lines = body.lines().collect::<Vec<_>>();
    let harness = Harness::new(|root| {
        std::fs::write(root.join("large.txt"), &body).expect("large file");
    });
    let streaming = harness.service(STREAMING_LIMIT, LargeFileReadMode::RequireRange);
    let in_memory = harness.service(IN_MEMORY_LIMIT, LargeFileReadMode::RequireRange);
    let mut shortest = usize::MAX;
    for (start, count) in [(0_u64, 50_u64), (1_000, 50), (7_000, 25), (9_000, 100)] {
        let range = TextReadRange::Lines { start_line_index: start, count };
        let streamed = streaming
            .read_text(harness.request("large.txt", range.clone()))
            .expect("streamed line range");
        let materialized = in_memory
            .read_text(harness.request("large.txt", range))
            .expect("whole-file line range");
        assert_eq!(streamed.text, materialized.text, "lines {start}..{}", start + count);
        let expected = expected_lines
            .iter()
            .skip(usize::try_from(start).expect("start"))
            .take(usize::try_from(count).expect("count"))
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(streamed.text, expected);
        assert_eq!(streamed.total_lines, materialized.total_lines);
        assert_eq!(streamed.total_characters, materialized.total_characters);
        assert_eq!(streamed.logical_hash, materialized.logical_hash);
        shortest = shortest.min(streamed.text.len());
    }
    // The whole file is never materialized: a 100-line range of a 1 MiB file is
    // four orders of magnitude smaller than the file.
    assert!(shortest < 8_192, "retained range stayed small: {shortest}");
}

#[test]
fn streaming_reads_keep_peak_retained_bytes_bounded_below_the_file_size() {
    let body = large_body(4 * 1024 * 1024);
    let file_bytes = u64::try_from(body.len()).expect("file length");
    let harness = Harness::new(|root| {
        std::fs::write(root.join("huge.txt"), &body).expect("huge file");
    });
    let resolver = harness.workspace.resolver(harness.workspace_id).expect("resolver");
    for range in [
        TextReadRange::Lines { start_line_index: 0, count: 20 },
        TextReadRange::Lines { start_line_index: 60_000, count: 20 },
        TextReadRange::Characters { start_character_offset: 1_500_000, count: 256 },
    ] {
        let path = WorkspacePath::parse("huge.txt").expect("path");
        let snapshot = match harness
            .workspace
            .buffers()
            .disk_text_snapshot_streaming(&resolver, &path, &range, None, STREAMING_LIMIT, CHUNK)
            .expect("streaming snapshot")
        {
            DiskTextSnapshot::Stream(snapshot) => snapshot,
            DiskTextSnapshot::UseSnapshot => panic!("expected a streamed read"),
        };
        assert_eq!(snapshot.logical_hash, ContentHash::blake3(body.as_bytes()));
        assert_eq!(snapshot.total_characters, u64::try_from(body.chars().count()).expect("chars"));
        assert_eq!(snapshot.total_lines, u64::try_from(body.lines().count()).expect("lines"));
        assert!(
            u64::try_from(snapshot.peak_retained_bytes).expect("peak") < file_bytes / 4,
            "peak retained {} must stay well below the {} byte file",
            snapshot.peak_retained_bytes,
            file_bytes,
        );
        assert!(
            snapshot.peak_retained_bytes <= CHUNK * 2,
            "peak retained {} must stay within two chunks of {CHUNK}",
            snapshot.peak_retained_bytes,
        );
    }
}

#[test]
fn large_file_policy_errors_and_ambiguous_encoding_are_unchanged() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("large.txt"), large_body(16 * 1024)).expect("large file");
        // A large file that is not valid UTF-8 and has no byte order mark.
        std::fs::write(root.join("binary.txt"), vec![0xff_u8; 16 * 1024]).expect("binary file");
    });
    let require_range = harness.service(STREAMING_LIMIT, LargeFileReadMode::RequireRange);
    assert!(matches!(
        require_range.read_text(harness.request("large.txt", TextReadRange::Full)),
        Err(FsToolError::LargeFileRangeRequired)
    ));
    let small_range = harness.service(STREAMING_LIMIT, LargeFileReadMode::SmallRangeOnly);
    assert!(matches!(
        small_range.read_text(harness.request(
            "large.txt",
            TextReadRange::Characters { start_character_offset: 0, count: 257 },
        )),
        Err(FsToolError::TextRangeTooLarge)
    ));
    // The same ambiguous-encoding failure the whole-file path reports.
    assert!(matches!(
        require_range.read_text(harness.request(
            "binary.txt",
            TextReadRange::Characters { start_character_offset: 0, count: 64 },
        )),
        Err(FsToolError::Workspace(aura_workspace::WorkspaceError::EncodingAmbiguous { .. }))
    ));
    // `AllowFull` still materializes the exact whole file.
    let allow_full = harness.service(STREAMING_LIMIT, LargeFileReadMode::AllowFull);
    let whole = allow_full
        .read_text(harness.request("large.txt", TextReadRange::Full))
        .expect("allow full read");
    assert_eq!(whole.text, large_body(16 * 1024));
}

#[test]
fn a_writable_editor_buffer_stays_authoritative_for_a_large_file() {
    let body = large_body(64 * 1024);
    let harness = Harness::new(|root| {
        std::fs::write(root.join("large.txt"), &body).expect("large file");
    });
    let service = harness.service(STREAMING_LIMIT, LargeFileReadMode::RequireRange);
    let path = WorkspacePath::parse("large.txt").expect("path");
    let opened = harness
        .workspace
        .open_buffer(BufferOpenCommand {
            workspace_id: harness.workspace_id,
            session_id: harness.session_id,
            path: path.clone(),
            writable: true,
        })
        .expect("open buffer");
    harness
        .workspace
        .edit_buffer(
            BufferEditCommand {
                workspace_id: harness.workspace_id,
                session_id: harness.session_id,
                path: path.clone(),
                reservation_id: opened.reservation_id.expect("reservation"),
                reservation_epoch: opened.reservation_epoch,
                buffer_revision: Revision::new(opened.buffer_revision.value() + 1),
                base_core_revision: opened.buffer_revision,
                content: "buffer line one\nbuffer line two\n".to_owned(),
                active_editing: true,
                ime_composition_active: false,
            },
            CorrelationId::new(),
        )
        .expect("edit buffer");
    let read = service
        .read_text(
            harness.request("large.txt", TextReadRange::Lines { start_line_index: 0, count: 2 }),
        )
        .expect("buffer read");
    assert_eq!(read.source, FileStorageSource::EditorBuffer);
    assert_eq!(read.text, "buffer line one\nbuffer line two");
    assert!(read.is_dirty);
    assert!(read.buffer_revision.is_some());
    assert_eq!(read.active_editor_session, Some(harness.session_id));
    assert!(!read.disk_may_be_stale);
}

#[test]
fn existing_file_encoding_and_line_endings_survive_a_write_and_new_files_are_utf8_lf() {
    let bom_text = "hello\nwörld\n";
    let utf16_text = "alpha\r\nbeta\r\n";
    let harness = Harness::new(|root| {
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend_from_slice(bom_text.as_bytes());
        std::fs::write(root.join("bom.txt"), &bom).expect("utf-8 bom file");
        std::fs::write(root.join("utf16.txt"), utf16le(utf16_text)).expect("utf-16 file");
    });
    let service = harness.service(IN_MEMORY_LIMIT, LargeFileReadMode::RequireRange);

    // Exactly what the model-facing `fs.write` handler passes: no explicit
    // encoding or line ending unless the file is being created.
    for (path, content, expected, encoding, line_ending) in [
        (
            "bom.txt",
            "goodbye\nwörld\n",
            vec![
                0xef, 0xbb, 0xbf, b'g', b'o', b'o', b'd', b'b', b'y', b'e', b'\n', b'w', 0xc3,
                0xb6, b'r', b'l', b'd', b'\n',
            ],
            TextEncoding::Utf8Bom,
            LineEnding::Lf,
        ),
        (
            "utf16.txt",
            "gamma\r\ndelta\r\n",
            utf16le("gamma\r\ndelta\r\n"),
            TextEncoding::Utf16Le,
            LineEnding::CrLf,
        ),
    ] {
        let before = std::fs::read(harness.root_path(path)).expect("before");
        service
            .write_file(
                FsWriteFileRequest {
                    workspace_id: harness.workspace_id,
                    session_id: harness.session_id,
                    target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                    content: (*content).to_owned(),
                    expected: FsExpectedState::FileHash { hash: ContentHash::blake3(&before) },
                    encoding: None,
                    line_ending: None,
                },
                CorrelationId::new(),
            )
            .expect("overwrite existing file");
        assert_eq!(std::fs::read(harness.root_path(path)).expect("after"), expected, "{path}");
        let read =
            service.read_text(harness.request(path, TextReadRange::Full)).expect("read back");
        assert_eq!(read.encoding, encoding, "{path} encoding preserved");
        assert_eq!(read.line_ending, line_ending, "{path} line ending preserved");
        assert_eq!(read.text, content, "{path} text preserved");
    }

    // A new file is created as UTF-8 with LF.
    service
        .write_file(
            FsWriteFileRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_id,
                target: WorkspaceFileTarget {
                    path: WorkspacePath::parse("created.txt").expect("path"),
                },
                content: "created\nwörld\n".to_owned(),
                expected: FsExpectedState::Absent,
                encoding: Some(TextEncoding::Utf8),
                line_ending: Some(LineEnding::Lf),
            },
            CorrelationId::new(),
        )
        .expect("create file");
    assert_eq!(
        std::fs::read(harness.root_path("created.txt")).expect("created"),
        "created\nwörld\n".as_bytes()
    );
    let created = service
        .read_text(harness.request("created.txt", TextReadRange::Full))
        .expect("read created");
    assert_eq!(created.encoding, TextEncoding::Utf8);
    assert_eq!(created.line_ending, LineEnding::Lf);
    // Without an explicit encoding a new file is still refused, not guessed.
    assert!(matches!(
        service.write_file(
            FsWriteFileRequest {
                workspace_id: harness.workspace_id,
                session_id: harness.session_id,
                target: WorkspaceFileTarget {
                    path: WorkspacePath::parse("refused.txt").expect("path"),
                },
                content: "refused\n".to_owned(),
                expected: FsExpectedState::Absent,
                encoding: None,
                line_ending: None,
            },
            CorrelationId::new(),
        ),
        Err(FsToolError::InvalidRequest(_))
    ));
}

// ---------------------------------------------------------------------------
// Encoding and metadata regressions.
//
// These four cases were reproduced as counter-examples against the first
// streaming implementation: it resolved the encoding from a byte order mark and
// otherwise assumed UTF-8, so an explicit choice was ignored, a UTF-8 mark was
// reported as plain UTF-8, and the line-ending convention was derived from the
// characters the requested range happened to retain. Each one is now compared
// against the whole-file path, which is the authority for what a read means.
// ---------------------------------------------------------------------------

/// An explicit Windows-1252 choice must not become an encoding failure just
/// because the file is large enough to take the streaming path. `0xe9` is `é` in
/// Windows-1252 and an invalid UTF-8 lead byte, so a reader that ignored the
/// explicit choice reported `EncodingAmbiguous`.
#[test]
fn review_explicit_windows1252_is_not_ambiguous_above_the_size_limit() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("legacy.txt"), vec![0xe9_u8; 6_000]).expect("legacy file");
    });
    assert_explicit_encoding_matches(
        &harness,
        "legacy.txt",
        TextReadRange::Characters { start_character_offset: 0, count: 2 },
        TextEncoding::Windows1252,
    );
    // The returned text is the decoded character, not the raw byte.
    let mut request = harness
        .request("legacy.txt", TextReadRange::Characters { start_character_offset: 0, count: 2 });
    request.explicit_encoding = Some(TextEncoding::Windows1252);
    let actual = streamed(&harness, request);
    assert_eq!(actual.text, "éé");
    assert_eq!(actual.encoding, TextEncoding::Windows1252);
}

/// An explicit UTF-16LE choice with no byte order mark must decode as UTF-16LE.
/// Assuming UTF-8 read the interleaved NUL bytes as characters and mislabelled
/// the encoding.
#[test]
fn review_explicit_utf16_without_a_byte_order_mark_is_honoured() {
    let harness = Harness::new(|root| {
        let bytes =
            "hello\r\n".repeat(600).encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        std::fs::write(root.join("utf16-no-bom.txt"), bytes).expect("utf16 file");
    });
    let mut request = harness.request(
        "utf16-no-bom.txt",
        TextReadRange::Characters { start_character_offset: 0, count: 5 },
    );
    request.explicit_encoding = Some(TextEncoding::Utf16Le);
    let expected = whole_file(&harness, request.clone());
    let actual = streamed(&harness, request);
    assert_eq!(facts(&actual), facts(&expected));
    assert_eq!(actual.text, "hello");
    assert_eq!(actual.encoding, TextEncoding::Utf16Le);
    assert_eq!(actual.total_characters, 4_200);
    assert_eq!(actual.total_lines, 600);
}

/// A consumed UTF-8 byte order mark is part of the answer: the whole-file path
/// reports `Utf8Bom`, so the streamed read must not report plain `Utf8`.
#[test]
fn review_a_utf8_byte_order_mark_is_reported_as_utf8_bom() {
    let harness = Harness::new(|root| {
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice("hello\n".repeat(1_000).as_bytes());
        std::fs::write(root.join("bom.txt"), bytes).expect("bom file");
    });
    let request = harness
        .request("bom.txt", TextReadRange::Characters { start_character_offset: 0, count: 5 });
    let expected = whole_file(&harness, request.clone());
    let actual = streamed(&harness, request);
    assert_eq!(facts(&actual), facts(&expected));
    assert_eq!(expected.encoding, TextEncoding::Utf8Bom);
    assert_eq!(actual.encoding, TextEncoding::Utf8Bom);
    // The mark itself is never part of the returned text.
    assert_eq!(actual.text, "hello");
}

/// The line-ending convention is a whole-file fact. A range that stops before the
/// bare carriage return must not hide it, and it must not turn a mixed file into
/// a pure line-feed one.
#[test]
fn review_line_endings_outside_the_returned_range_are_still_counted() {
    let harness = Harness::new(|root| {
        // A long first line, then a bare carriage return, a line feed, and a tail:
        // the range reads only the first character, so both later breaks are far
        // outside it.
        std::fs::write(root.join("ending.txt"), format!("{}\rb\nc", "a".repeat(6_000)))
            .expect("ending file");
    });
    let request = harness
        .request("ending.txt", TextReadRange::Characters { start_character_offset: 0, count: 1 });
    let expected = whole_file(&harness, request.clone());
    let actual = streamed(&harness, request);
    assert_eq!(facts(&actual), facts(&expected));
    assert_eq!(expected.line_ending, LineEnding::Mixed);
    assert_eq!(actual.line_ending, LineEnding::Mixed);
    assert_eq!(actual.text, "a");
    assert_eq!(actual.total_lines, expected.total_lines);
}

/// Every supported encoding, reached both explicitly and by detection, must agree
/// with the whole-file path for the same request.
///
/// The matrix keeps the two situations distinct on purpose: a byte order mark is
/// detected on its own, while a mark-less UTF-16 file is only readable when the
/// caller names the encoding, because nothing in the bytes identifies it and the
/// whole-file path refuses to guess.
#[test]
fn every_supported_encoding_agrees_between_both_read_paths() {
    let body = "alpha\nbéta\n".repeat(400);
    let harness = Harness::new(|root| {
        std::fs::write(root.join("plain.txt"), &body).expect("utf8");
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend_from_slice(body.as_bytes());
        std::fs::write(root.join("utf8-bom.txt"), bom).expect("utf8 bom");
        let mut utf16le = vec![0xff, 0xfe];
        for unit in body.encode_utf16() {
            utf16le.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(root.join("utf16le-bom.txt"), utf16le).expect("utf16le bom");
        let mut utf16be = vec![0xfe, 0xff];
        for unit in body.encode_utf16() {
            utf16be.extend_from_slice(&unit.to_be_bytes());
        }
        std::fs::write(root.join("utf16be-bom.txt"), utf16be).expect("utf16be bom");
        let utf16le_bare = body.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        std::fs::write(root.join("utf16le-bare.txt"), utf16le_bare)
            .expect("utf16le without a mark");
        std::fs::write(root.join("cp1252.txt"), vec![0xe9_u8; 6_000]).expect("cp1252");
    });
    let ranges = [
        TextReadRange::Characters { start_character_offset: 0, count: 7 },
        TextReadRange::Characters { start_character_offset: 3, count: 5 },
        TextReadRange::Lines { start_line_index: 0, count: 3 },
        TextReadRange::Lines { start_line_index: 2, count: 2 },
    ];

    // Detection: no explicit choice, so only the marked files have an answer.
    for file in ["plain.txt", "utf8-bom.txt", "utf16le-bom.txt", "utf16be-bom.txt"] {
        for range in &ranges {
            assert_streaming_matches_whole_file(&harness, harness.request(file, range.clone()));
        }
    }
    // Explicit choices, including a mark-less UTF-16 file and the encoding that
    // detection can never reach.
    for (file, encoding) in [
        ("plain.txt", TextEncoding::Utf8),
        ("plain.txt", TextEncoding::Utf8Bom),
        ("utf8-bom.txt", TextEncoding::Utf8Bom),
        ("utf8-bom.txt", TextEncoding::Utf8),
        ("utf16le-bom.txt", TextEncoding::Utf16Le),
        ("utf16be-bom.txt", TextEncoding::Utf16Be),
        ("utf16le-bare.txt", TextEncoding::Utf16Le),
        ("cp1252.txt", TextEncoding::Windows1252),
    ] {
        for range in &ranges {
            assert_explicit_encoding_matches(&harness, file, range.clone(), encoding);
        }
    }
}

/// A mark-less UTF-16 file is not detected as UTF-16 by either path.
///
/// `"h\0e\0l\0l\0o\0"` is valid UTF-8 — the NUL bytes are characters — so the
/// whole-file decoder reads it as UTF-8 text and only an explicit encoding turns
/// it into UTF-16. This test records that shared behaviour so the streaming path
/// can never drift into being "helpfully" different.
#[test]
fn a_mark_less_utf16_file_reads_as_utf8_on_both_paths_without_an_explicit_choice() {
    let harness = Harness::new(|root| {
        let bytes =
            "hello\r\n".repeat(600).encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>();
        std::fs::write(root.join("bare.txt"), bytes).expect("mark-less utf16");
    });
    let request = harness
        .request("bare.txt", TextReadRange::Characters { start_character_offset: 0, count: 5 });
    assert_streaming_matches_whole_file(&harness, request.clone());
    let actual = streamed(&harness, request);
    assert_eq!(actual.encoding, TextEncoding::Utf8);
    assert_eq!(actual.text, "h\u{0}e\u{0}l");
}

/// A file that is not valid UTF-8 and carries no mark stays ambiguous unless the
/// caller names an encoding. Detection must not guess Windows-1252 and silently
/// substitute replacement characters, so both paths refuse it the same way.
#[test]
fn ambiguous_bytes_without_a_mark_stay_ambiguous_on_both_paths() {
    let harness = Harness::new(|root| {
        std::fs::write(root.join("ambiguous.bin"), vec![0xe9_u8; 6_000]).expect("ambiguous file");
    });
    let request = harness.request(
        "ambiguous.bin",
        TextReadRange::Characters { start_character_offset: 0, count: 4 },
    );
    let whole_file_error = harness
        .service(IN_MEMORY_LIMIT, LargeFileReadMode::RequireRange)
        .read_text(request.clone())
        .expect_err("the whole-file path must refuse these bytes rather than guess");
    let streamed_error = harness
        .service(STREAMING_LIMIT, LargeFileReadMode::RequireRange)
        .read_text(request)
        .expect_err("the streaming path must refuse them the same way");
    assert!(
        matches!(
            whole_file_error,
            FsToolError::Workspace(aura_workspace::WorkspaceError::EncodingAmbiguous { .. })
        ),
        "unexpected whole-file error: {whole_file_error:?}"
    );
    assert!(
        matches!(
            streamed_error,
            FsToolError::Workspace(aura_workspace::WorkspaceError::EncodingAmbiguous { .. })
        ),
        "unexpected streaming error: {streamed_error:?}"
    );
}
