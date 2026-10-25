use std::sync::Arc;

use aura_workspace_domain::{
    FileStorageSource, FsSearchIncompleteReason, LogicalFileSnapshot, TextEncoding,
};
use grep_regex::RegexMatcher;
use grep_searcher::SearcherBuilder;

use super::{
    budget::ByteBudget,
    cancel::SearchCancel,
    candidates::SearchCandidate,
    diagnostics::StageDiagnosticsHandle,
    policy::{LINE_HEAP_LIMIT, MAX_FILE_BYTES},
    sink::MatchSink,
    types::{FileOutcome, to_wire_match},
};
use crate::FsToolError;

/// One in-memory scan for one candidate, carrying everything the lanes share.
pub(crate) struct OverlayScanRequest<'a> {
    pub(crate) snapshot: LogicalFileSnapshot,
    pub(crate) candidate: &'a SearchCandidate,
    pub(crate) matcher: &'a RegexMatcher,
    pub(crate) budget: &'a Arc<ByteBudget>,
    pub(crate) reserved: u64,
    pub(crate) cancel: &'a SearchCancel,
    pub(crate) match_cap: usize,
    pub(crate) stages: &'a StageDiagnosticsHandle,
}

pub(crate) fn scan_overlay(request: OverlayScanRequest<'_>) -> Result<FileOutcome, FsToolError> {
    let OverlayScanRequest {
        snapshot,
        candidate,
        matcher,
        budget,
        reserved,
        cancel,
        match_cap,
        stages,
    } = request;
    cancel.mark_started();
    let billed = u64::try_from(snapshot.bytes.len()).unwrap_or(u64::MAX);
    if billed > MAX_FILE_BYTES {
        budget.release_attempt(0, reserved);
        return Ok(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            0,
            vec![FsSearchIncompleteReason::FileTooLarge],
        ));
    }
    if billed > reserved {
        budget.release_attempt(0, reserved);
        return Ok(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            0,
            vec![FsSearchIncompleteReason::ScanByteBudget],
        ));
    }
    budget.release_attempt(billed, reserved);
    if cancel.is_cancelled() {
        return Ok(FileOutcome {
            ordinal: candidate.ordinal,
            path: candidate.path.clone(),
            matches: Vec::new(),
            scanned: true,
            skipped: false,
            bytes: billed,
            reasons: Vec::new(),
        });
    }
    // The snapshot's producing layer already established the encoding. Decode
    // with that declaration only; there is no guessing fallback.
    let Some(encoding) = snapshot.encoding else {
        return Ok(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            billed,
            vec![FsSearchIncompleteReason::UnsupportedEncoding],
        ));
    };
    let text = match decode_declared(&snapshot.bytes, encoding) {
        Ok(text) => text,
        Err(_) => {
            return Ok(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                billed,
                vec![FsSearchIncompleteReason::UnsupportedEncoding],
            ));
        }
    };
    let mut sink = MatchSink::new(
        candidate.path.clone(),
        matcher.clone(),
        cancel.clone(),
        match_cap,
        Arc::clone(stages),
    );
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .multi_line(false)
        .binary_detection(grep_searcher::BinaryDetection::none())
        .bom_sniffing(false)
        .heap_limit(Some(LINE_HEAP_LIMIT))
        .build();
    if let Err(error) = searcher.search_slice(matcher, text.as_bytes(), &mut sink) {
        if cancel.is_cancelled() {
            return Ok(FileOutcome {
                ordinal: candidate.ordinal,
                path: candidate.path.clone(),
                matches: Vec::new(),
                scanned: true,
                skipped: false,
                bytes: billed,
                reasons: Vec::new(),
            });
        }
        return Err(FsToolError::Internal(format!("buffer search failed: {error}")));
    }
    if sink.unsupported {
        return Ok(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            billed,
            vec![FsSearchIncompleteReason::UnsupportedEncoding],
        ));
    }
    if sink.binary {
        return Ok(FileOutcome {
            ordinal: candidate.ordinal,
            path: candidate.path.clone(),
            matches: Vec::new(),
            scanned: true,
            skipped: true,
            bytes: billed,
            reasons: Vec::new(),
        });
    }
    let matches = sink
        .prepared
        .into_iter()
        .map(|prepared| {
            to_wire_match(
                prepared,
                FileStorageSource::EditorBuffer,
                snapshot.logical_hash.clone(),
                snapshot.buffer_revision,
                candidate.ignored,
                candidate.generated,
            )
        })
        .collect();
    Ok(FileOutcome {
        ordinal: candidate.ordinal,
        path: candidate.path.clone(),
        matches,
        scanned: true,
        skipped: false,
        bytes: billed,
        reasons: Vec::new(),
    })
}

/// Decode a snapshot with the encoding the producing layer declared. The
/// workspace decoder is the single authority; an undeclared or mismatched
/// encoding is unsupported rather than silently widened.
fn decode_declared(bytes: &[u8], encoding: TextEncoding) -> Result<String, ()> {
    aura_workspace::decode_text_with_encoding(bytes, encoding)
        .map(|decoded| decoded.text)
        .map_err(|_| ())
}
