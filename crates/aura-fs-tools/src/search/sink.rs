use std::{io, sync::atomic::Ordering};

use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkMatch};

use super::{
    cancel::SearchCancel,
    diagnostics::StageDiagnosticsHandle,
    excerpt::{char_boundaries, excerpt_around_bytes},
    matcher::find_utf8_matches,
    types::PreparedMatch,
};

pub(crate) struct MatchSink {
    path: aura_domain_common::WorkspacePath,
    pub(crate) prepared: Vec<PreparedMatch>,
    pub(crate) unsupported: bool,
    pub(crate) binary: bool,
    cancel: SearchCancel,
    matcher: RegexMatcher,
    match_cap: usize,
    stages: StageDiagnosticsHandle,
}

impl MatchSink {
    pub(crate) fn new(
        path: aura_domain_common::WorkspacePath,
        matcher: RegexMatcher,
        cancel: SearchCancel,
        match_cap: usize,
        stages: StageDiagnosticsHandle,
    ) -> Self {
        Self {
            path,
            prepared: Vec::new(),
            unsupported: false,
            binary: false,
            cancel,
            matcher,
            match_cap: match_cap.max(1),
            stages,
        }
    }
}

impl Sink for MatchSink {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if self.cancel.is_cancelled() {
            return Ok(false);
        }
        self.cancel.mark_started();
        if self.prepared.len() >= self.match_cap {
            return Ok(true);
        }
        let Ok(line) = std::str::from_utf8(mat.bytes()) else {
            self.unsupported = true;
            return Ok(false);
        };
        let line = line.trim_end_matches(['\r', '\n']);
        let line_index = mat.line_number().unwrap_or(1).saturating_sub(1);
        let remaining = self.match_cap.saturating_sub(self.prepared.len());
        let found = find_utf8_matches(&self.matcher, line, &self.cancel, remaining)
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.stages.matched_lines.fetch_add(1, Ordering::Relaxed);
        self.stages
            .matched_line_bytes
            .fetch_add(u64::try_from(line.len()).unwrap_or(u64::MAX), Ordering::Relaxed);
        self.stages
            .matched_hits
            .fetch_add(u64::try_from(found.len()).unwrap_or(u64::MAX), Ordering::Relaxed);
        // Every hit needs the same walks over the line: once to turn byte
        // offsets into character offsets and once to find the excerpt window.
        // The offsets are resolved in one ordered walk and the window is built
        // outwards from the hit, so projecting a long dense line stays linear in
        // the line plus the hits instead of hits × line length.
        let total_chars = line.chars().count();
        let boundaries = char_boundaries(line, &found);
        let line_bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);
        self.stages.char_offset_bytes.fetch_add(line_bytes, Ordering::Relaxed);
        for ((start, end), (start_chars, end_chars)) in found.iter().zip(boundaries) {
            if self.cancel.is_cancelled() {
                return Ok(false);
            }
            if self.prepared.len() >= self.match_cap {
                break;
            }
            let length = end_chars.saturating_sub(start_chars);
            let excerpt =
                excerpt_around_bytes(line, *start, *end, start_chars, length, total_chars);
            self.prepared.push(PreparedMatch {
                path: self.path.clone(),
                line_index,
                column_character_offset: u64::try_from(start_chars).unwrap_or(u64::MAX),
                match_character_length: u64::try_from(length).unwrap_or(u64::MAX),
                excerpt: excerpt.text,
                excerpt_start_character_offset: excerpt.start_character_offset,
                excerpt_truncated_before: excerpt.truncated_before,
                excerpt_truncated_after: excerpt.truncated_after,
            });
        }
        Ok(true)
    }

    fn binary_data(
        &mut self,
        _searcher: &Searcher,
        _binary_byte_offset: u64,
    ) -> Result<bool, Self::Error> {
        self.binary = true;
        Ok(false)
    }
}
