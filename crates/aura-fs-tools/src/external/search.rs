//! Text search under one already-authorized external directory.

use super::*;

/// One authorized external directory to search for text.
pub struct ExternalSearchTarget {
    /// The session grant that authorized this target, when a grant did.
    pub grant_id: Option<ExternalGrantId>,
    pub session_id: SessionId,
    /// The absolute directory path as the user sees it.
    pub display_path: String,
    /// The directory relative to the grant root.
    pub relative_path: WorkspacePath,
    pub directory: VerifiedFile,
}

pub struct FsExternalSearchRequest {
    pub target: ExternalSearchTarget,
    pub pattern: FsSearchPattern,
    pub recursive: bool,
    /// Only the files this glob names are searched, read as the workspace search reads it.
    pub path_glob: Option<String>,
    pub limit: u32,
    /// How many directory entries this call may examine in total.
    pub scan_budget: u64,
    /// Files larger than this are not read, and the result says it is incomplete.
    pub max_file_bytes: u64,
    /// How many bytes of file content this call may read in total.
    pub byte_budget: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalSearchMatch {
    pub relative_path: WorkspacePath,
    pub name: String,
    /// One-based line number, as a reader counts lines.
    pub line_number: u64,
    /// Where the match starts inside the line, and how many characters it covers.
    pub column_character_offset: u64,
    pub match_character_length: u64,
    pub line_text: String,
    /// True when the line was longer than the excerpt kept here.
    pub line_truncated: bool,
    /// Hash of the file content this match came from.
    pub logical_hash: ContentHash,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalSearchResult {
    pub grant_id: Option<ExternalGrantId>,
    pub display_path: String,
    pub matches: Vec<FsExternalSearchMatch>,
    pub scanned_entries: u64,
    /// Files whose content was actually read.
    pub scanned_file_count: u64,
    pub scanned_bytes: u64,
    /// True only when the walk finished and every file it saw was searched.
    pub complete: bool,
    pub skipped_too_large: u64,
    pub skipped_not_text: u64,
}

impl FsToolService {
    /// Searches text under one already-authorized external directory.
    ///
    /// An external tree has no index, so this reads real files: a bounded walk in path
    /// order that never follows a reparse point, and a bounded read of each file it
    /// examines. Every bound is real — entries examined, bytes read — so a wide or deep
    /// tree cannot turn one call into an unbounded scan, and `complete` is true only when
    /// the walk finished and every file it saw was searched. A file that is too large,
    /// or that is not text, is counted and reported instead of being silently skipped:
    /// "no match" must never be claimed for content nobody read.
    pub fn search_external(
        &self,
        request: FsExternalSearchRequest,
        cancel: Option<&crate::search::SearchCancel>,
    ) -> Result<FsExternalSearchResult, FsToolError> {
        let FsExternalSearchRequest {
            target,
            pattern,
            recursive,
            path_glob,
            limit,
            scan_budget,
            max_file_bytes,
            byte_budget,
        } = request;
        let final_path = target.directory.info().final_path.clone();
        if !target.directory.info().is_directory {
            return Err(FsToolError::InvalidRequest(
                "an external search root must name a directory".into(),
            ));
        }
        let glob = path_glob.as_deref().map(crate::search::read_path_glob).transpose()?;
        let identity = aura_workspace::domain_identity(&target.directory.info().identity);
        let matcher = match &pattern {
            FsSearchPattern::Literal { text, case_sensitive } => {
                let mut builder = regex::RegexBuilder::new(&regex::escape(text));
                builder.case_insensitive(!case_sensitive);
                builder.build().map_err(|error| FsToolError::InvalidRequest(error.to_string()))?
            }
            FsSearchPattern::Regex { pattern, case_sensitive } => {
                let mut builder = regex::RegexBuilder::new(pattern);
                builder.case_insensitive(!case_sensitive);
                builder.build().map_err(|error| FsToolError::InvalidRequest(error.to_string()))?
            }
        };
        let mut pending = std::collections::BinaryHeap::new();
        let mut read_budget = scan_budget;
        let root_info = target.directory.info().clone();
        let root_key = identity_key(&root_info.identity);
        let mut listings = ExternalWalkMemory::default();
        let listed = push_children(&mut ChildListing {
            memory: &mut listings,
            root_key: &root_key,
            root_info: &root_info,
            resume: false,
            pending: &mut pending,
            directory: &target.relative_path,
            absolute: &final_path,
            budget: read_budget,
            consumed: &[],
            cancel,
        })?;
        read_budget = read_budget.saturating_sub(listed.progress);
        let mut read_truncated = listed.truncated;
        let mut scanned_entries = 0_u64;
        let mut scanned_file_count = 0_u64;
        let mut scanned_bytes = 0_u64;
        let mut matches = Vec::new();
        let mut skipped_too_large = 0_u64;
        let mut skipped_not_text = 0_u64;
        let mut complete = true;
        while let Some(candidate) = pending.pop() {
            if cancel.is_some_and(crate::search::SearchCancel::is_cancelled) {
                return Err(FsToolError::Cancelled);
            }
            if candidate.kind == FsNodeKind::Directory {
                if recursive {
                    let child_absolute = candidate.absolute.clone();
                    let listed = push_children(&mut ChildListing {
                        memory: &mut listings,
                        root_key: &root_key,
                        root_info: &root_info,
                        resume: false,
                        pending: &mut pending,
                        directory: &candidate.relative_path,
                        absolute: &child_absolute,
                        budget: read_budget,
                        consumed: &[],
                        cancel,
                    })?;
                    read_budget = read_budget.saturating_sub(listed.progress);
                    read_truncated = read_truncated || listed.truncated;
                }
                continue;
            }
            // A file the glob does not name is outside this search: it is not read and it does
            // not count against the budget or make the result incomplete.
            if glob
                .as_ref()
                .is_some_and(|glob| !glob.matches(&candidate.relative_path, &target.relative_path))
            {
                continue;
            }
            scanned_entries = scanned_entries.saturating_add(1);
            if scanned_entries > scan_budget {
                complete = false;
                break;
            }
            // A reparse point is reported but never followed or read.
            if candidate.kind == FsNodeKind::ReparsePoint {
                continue;
            }
            let size = candidate.byte_length.unwrap_or(0);
            if size > max_file_bytes {
                skipped_too_large = skipped_too_large.saturating_add(1);
                complete = false;
                continue;
            }
            if scanned_bytes.saturating_add(size) > byte_budget {
                complete = false;
                break;
            }
            // The bytes come through the verified handle the platform opened, not from the
            // path the listing spelled: a name that was replaced between the listing and
            // this read is reported as an unfinished search instead of being searched as
            // if it were the file the walk saw.
            let Ok(mut file) = aura_windows_platform::open_verified_existing(
                &candidate.absolute,
                aura_windows_platform::NativeFileAccess::Read,
            ) else {
                // A file that cannot be read is not a file that does not match.
                complete = false;
                continue;
            };
            if file.info().is_directory
                || !final_path_within(&root_info, file.info())
                || !listings
                    .recall(&root_key, &candidate.relative_path)
                    .is_some_and(|seen| seen.identity == file.info().identity)
                || !crate::read_scope::allows(file.info())
            {
                complete = false;
                continue;
            }
            let Some(read) = read_via_handle(&mut file, size) else {
                complete = false;
                continue;
            };
            // The bytes really read are billed whether or not they decode, so a search
            // full of non-text files cannot read them for free.
            scanned_bytes = scanned_bytes.saturating_add(read.bytes_read);
            if read.bytes_read != size {
                // The file is not the size the listing saw: it changed while it was
                // searched, so nothing about it is settled.
                complete = false;
                continue;
            }
            let Some(decoded) = crate::read::decode_snapshot(&read.bytes, None).ok() else {
                skipped_not_text = skipped_not_text.saturating_add(1);
                complete = false;
                continue;
            };
            scanned_file_count = scanned_file_count.saturating_add(1);
            let text = decoded.text;
            let logical_hash = ContentHash::blake3(&read.bytes);
            for (index, line) in text.lines().enumerate() {
                let spans: Vec<(usize, usize)> =
                    matcher.find_iter(line).map(|found| (found.start(), found.end())).collect();
                if spans.is_empty() {
                    continue;
                }
                // One ordered walk resolves every boundary on the line, and the excerpt is
                // the window around the match, so a hit far into a long line is projected
                // with its own text instead of the first bytes of the line.
                let boundaries = crate::search::excerpt::char_boundaries(line, &spans);
                let total_chars = line.chars().count();
                for ((start, end), (start_chars, end_chars)) in spans.iter().zip(&boundaries) {
                    let length = end_chars.saturating_sub(*start_chars);
                    let excerpt = crate::search::excerpt::excerpt_around_bytes(
                        line,
                        *start,
                        *end,
                        *start_chars,
                        length,
                        total_chars,
                    );
                    matches.push(FsExternalSearchMatch {
                        relative_path: candidate.relative_path.clone(),
                        name: candidate.name.clone(),
                        line_number: u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1),
                        column_character_offset: u64::try_from(*start_chars).unwrap_or(u64::MAX),
                        match_character_length: u64::try_from(length).unwrap_or(u64::MAX),
                        line_truncated: excerpt.truncated_before || excerpt.truncated_after,
                        line_text: excerpt.text,
                        logical_hash: logical_hash.clone(),
                    });
                    if matches.len() >= limit as usize {
                        // The page is full; the walk did not finish.
                        complete = false;
                        break;
                    }
                }
                if matches.len() >= limit as usize {
                    break;
                }
            }
            if matches.len() >= limit as usize {
                break;
            }
        }
        if read_truncated {
            complete = false;
        }
        ensure_same_file(&final_path, &identity)?;
        Ok(FsExternalSearchResult {
            grant_id: target.grant_id,
            display_path: target.display_path,
            matches,
            scanned_entries,
            scanned_file_count,
            scanned_bytes,
            complete,
            skipped_too_large,
            skipped_not_text,
        })
    }
}
