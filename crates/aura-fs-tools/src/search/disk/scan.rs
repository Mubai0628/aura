use super::*;

pub(super) fn scan_disk_once(input: DiskOnce<'_>) -> Result<DiskScan, FsToolError> {
    let DiskOnce {
        resolver,
        candidate,
        matcher,
        budget,
        reserved,
        cancel,
        match_cap,
        prior,
        stages,
    } = input;
    let held = Held::new(budget, candidate.ordinal, reserved);
    enter_test_gate(candidate, cancel);
    if cancel.is_cancelled() {
        held.release();
        return Ok(DiskScan::Outcome(FileOutcome {
            ordinal: candidate.ordinal,
            path: candidate.path.clone(),
            matches: Vec::new(),
            scanned: false,
            skipped: true,
            bytes: prior.get(),
            reasons: Vec::new(),
        }));
    }
    let mut source = {
        stages.open_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let opened = {
            let _open = StageTimer::start(&stages.open_micros);
            resolver.open_search_source(&candidate.path)
        };
        match opened {
            Ok(value) => value,
            Err(error) => {
                held.release();
                return map_open_error(error, candidate, prior.get());
            }
        }
    };
    if !crate::read_scope::allows(source.info()) {
        held.release();
        return Ok(DiskScan::Outcome(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            prior.get(),
            vec![FsSearchIncompleteReason::ScopeExcluded],
        )));
    }
    let start_identity = source.identity().clone();
    let writers_denied = source.writers_denied();
    // The opened handle is authoritative for size; a stale catalog hint must not
    // decide whether the file is searched.
    let size = match source.file_mut().metadata() {
        Ok(meta) => meta.len(),
        Err(_) => {
            held.release();
            return Ok(DiskScan::Outcome(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                prior.get(),
                vec![FsSearchIncompleteReason::FileTemporarilyUnavailable],
            )));
        }
    };
    if size > MAX_FILE_BYTES {
        held.release();
        return Ok(DiskScan::Outcome(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            prior.get(),
            vec![FsSearchIncompleteReason::FileTooLarge],
        )));
    }
    // The probe reads at most what the file's allowance covers; the allowance
    // is granted before any byte reaches the file handle. A file whose turn has
    // not arrived yet waits for it: "not my turn yet" is never budget exhaustion
    // and the wait ends only when the turn arrives or the search is cancelled
    // (see `policy::BUDGET_WAIT_POLL` for the liveness argument).
    let mut probe = vec![0_u8; PROBE_BYTES];
    let probe_allow = loop {
        match held.take_for_read(probe.len()) {
            Allowance::Granted(allow) => break allow,
            Allowance::WaitingForPredecessor => {
                if cancel.is_cancelled() {
                    held.release();
                    return Ok(DiskScan::Outcome(cancelled_outcome(candidate, prior.get())));
                }
                std::thread::sleep(BUDGET_WAIT_POLL);
            }
            Allowance::Exhausted => {
                held.release();
                return Ok(DiskScan::Outcome(FileOutcome::skipped(
                    candidate.ordinal,
                    candidate.path.clone(),
                    prior.get(),
                    vec![FsSearchIncompleteReason::ScanByteBudget],
                )));
            }
        }
    };
    if probe_allow == 0 {
        held.release();
        return Ok(DiskScan::Outcome(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            prior.get(),
            vec![FsSearchIncompleteReason::ScanByteBudget],
        )));
    }
    let read = match source.file_mut().read(&mut probe[..probe_allow]) {
        Ok(value) => value,
        Err(_) => {
            held.release();
            return Ok(DiskScan::Outcome(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                prior.get(),
                vec![FsSearchIncompleteReason::FileTemporarilyUnavailable],
            )));
        }
    };
    cancel.mark_started();
    probe.truncate(read);
    let probe_read = u64::try_from(read).unwrap_or(0);
    held.charge(probe_read);
    let prior = prior.accumulate(probe_read);
    // An empty read of a file the catalog recorded with content is a concurrent
    // truncation, not an empty file: reporting "scanned, no matches" would look
    // like a complete answer for a file whose content was never seen. Fail
    // closed and let the caller retry or report the change.
    if probe_read == 0 && size == 0 && candidate.byte_length.unwrap_or(0) > 0 {
        held.release();
        return Ok(DiskScan::Outcome(FileOutcome::skipped(
            candidate.ordinal,
            candidate.path.clone(),
            prior.get(),
            vec![FsSearchIncompleteReason::FileChangedDuringSearch],
        )));
    }
    match classify_probe(&probe, candidate.catalog_encoding) {
        ProbeKind::Binary => {
            held.release();
            Ok(DiskScan::Outcome(FileOutcome {
                ordinal: candidate.ordinal,
                path: candidate.path.clone(),
                matches: Vec::new(),
                scanned: true,
                skipped: true,
                bytes: prior.get(),
                reasons: Vec::new(),
            }))
        }
        ProbeKind::Unsupported => {
            held.release();
            Ok(DiskScan::Outcome(FileOutcome::skipped(
                candidate.ordinal,
                candidate.path.clone(),
                prior.get(),
                vec![FsSearchIncompleteReason::UnsupportedEncoding],
            )))
        }
        ProbeKind::Text(encoding) => {
            if source.file_mut().seek(SeekFrom::Start(0)).is_err() {
                held.release();
                return Ok(DiskScan::Outcome(FileOutcome::skipped(
                    candidate.ordinal,
                    candidate.path.clone(),
                    prior.get(),
                    vec![FsSearchIncompleteReason::FileTemporarilyUnavailable],
                )));
            }
            let mut sink = MatchSink::new(
                candidate.path.clone(),
                matcher.clone(),
                cancel.clone(),
                match_cap,
                Arc::clone(stages),
            );
            let (scanned_bytes, exhausted, too_large, search, scan_hash) = {
                let mut metered = MeteredReader {
                    inner: source.file_mut(),
                    held: &held,
                    hasher: blake3::Hasher::new(),
                    read: 0,
                    expected: size.min(MAX_FILE_BYTES),
                    cancel: cancel.clone(),
                    exhausted: false,
                    too_large: false,
                };
                let mut searcher = disk_searcher(encoding);
                let search = {
                    let _read = StageTimer::start(&stages.read_micros);
                    searcher.search_reader(matcher, &mut metered, &mut sink)
                };
                let scanned_bytes = metered.read;
                let exhausted = metered.exhausted;
                let too_large = metered.too_large;
                let scan_hash = ContentHash {
                    algorithm: aura_domain_common::ContentHashAlgorithm::Blake3,
                    hex: metered.hasher.finalize().to_hex().to_string(),
                };
                drop(metered);
                (scanned_bytes, exhausted, too_large, search, scan_hash)
            };
            stages.scanned_bytes.fetch_add(scanned_bytes, std::sync::atomic::Ordering::Relaxed);
            let scanned_total = prior.accumulate(scanned_bytes);
            if cancel.is_cancelled() {
                held.release();
                return Ok(DiskScan::Outcome(FileOutcome {
                    ordinal: candidate.ordinal,
                    path: candidate.path.clone(),
                    matches: Vec::new(),
                    scanned: false,
                    skipped: true,
                    bytes: scanned_total.get(),
                    reasons: Vec::new(),
                }));
            }
            if too_large {
                held.release();
                return Ok(DiskScan::Outcome(FileOutcome::skipped(
                    candidate.ordinal,
                    candidate.path.clone(),
                    scanned_total.get(),
                    vec![FsSearchIncompleteReason::FileTooLarge],
                )));
            }

            if exhausted {
                held.release();
                return Ok(DiskScan::Outcome(FileOutcome::skipped(
                    candidate.ordinal,
                    candidate.path.clone(),
                    scanned_total.get(),
                    vec![FsSearchIncompleteReason::ScanByteBudget],
                )));
            }
            if let Err(error) = search {
                held.release();
                return Err(FsToolError::Io {
                    action: "search",
                    path: resolver.absolute_path(&candidate.path),
                    source: error,
                });
            }
            if sink.unsupported {
                held.release();
                return Ok(DiskScan::Outcome(FileOutcome::skipped(
                    candidate.ordinal,
                    candidate.path.clone(),
                    scanned_total.get(),
                    vec![FsSearchIncompleteReason::UnsupportedEncoding],
                )));
            }
            if sink.binary {
                held.release();
                return Ok(DiskScan::Outcome(FileOutcome {
                    ordinal: candidate.ordinal,
                    path: candidate.path.clone(),
                    matches: Vec::new(),
                    scanned: true,
                    skipped: true,
                    bytes: scanned_total.get(),
                    reasons: Vec::new(),
                }));
            }
            let verified = {
                let _verify = StageTimer::start(&stages.verify_micros);
                stages.verify_calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let outcome = verify_consistency(ConsistencyInput {
                    candidate,
                    source: &mut source,
                    start_identity: &start_identity,
                    writers_denied,
                    scanned_bytes,
                    scan_hash: &scan_hash,
                    held: &held,
                    cancel,
                });
                let (Verify::Ok { hashed_bytes, .. }
                | Verify::Changed { extra_bytes: hashed_bytes }
                | Verify::BudgetExhausted { extra_bytes: hashed_bytes }
                | Verify::Unavailable { extra_bytes: hashed_bytes }
                | Verify::Cancelled { extra_bytes: hashed_bytes }) = &outcome;
                stages.verify_bytes.fetch_add(*hashed_bytes, std::sync::atomic::Ordering::Relaxed);
                outcome
            };
            match verified {
                Verify::BudgetExhausted { extra_bytes } => {
                    // A re-hash that ran out of allowance is a budget outcome,
                    // reported separately from I/O unavailability.
                    let total = scanned_total.accumulate(extra_bytes);
                    held.release();
                    Ok(DiskScan::Outcome(FileOutcome::skipped(
                        candidate.ordinal,
                        candidate.path.clone(),
                        total.get(),
                        vec![FsSearchIncompleteReason::ScanByteBudget],
                    )))
                }
                Verify::Ok { hash, hashed_bytes } => {
                    let total = scanned_total.accumulate(hashed_bytes);
                    held.release();
                    let matches = sink
                        .prepared
                        .into_iter()
                        .map(|prepared| {
                            to_wire_match(
                                prepared,
                                FileStorageSource::Disk,
                                hash.clone(),
                                None,
                                candidate.ignored,
                                candidate.generated,
                            )
                        })
                        .collect();
                    Ok(DiskScan::Outcome(FileOutcome {
                        ordinal: candidate.ordinal,
                        path: candidate.path.clone(),
                        matches,
                        scanned: true,
                        skipped: false,
                        bytes: total.get(),
                        reasons: Vec::new(),
                    }))
                }
                Verify::Changed { extra_bytes } => {
                    let total = scanned_total.accumulate(extra_bytes);
                    held.release();
                    Ok(DiskScan::Retry { bytes: total.get() })
                }
                Verify::Cancelled { extra_bytes } => {
                    let total = scanned_total.accumulate(extra_bytes);
                    held.release();
                    Ok(DiskScan::Outcome(cancelled_outcome(candidate, total.get())))
                }
                Verify::Unavailable { extra_bytes } => {
                    let total = scanned_total.accumulate(extra_bytes);
                    held.release();
                    Ok(DiskScan::Outcome(FileOutcome::skipped(
                        candidate.ordinal,
                        candidate.path.clone(),
                        total.get(),
                        vec![FsSearchIncompleteReason::FileTemporarilyUnavailable],
                    )))
                }
            }
        }
    }
}
