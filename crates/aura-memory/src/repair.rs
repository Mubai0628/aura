use crate::*;
use aura_domain_common::{MemoryId, UtcTimestamp, WorkspacePath};
use aura_fs_tools::vault::VaultChange;

pub(crate) struct PendingRepair {
    owner: (String, Option<aura_domain_common::SessionId>),
    at: std::time::Instant,
    preview: MemoryRepairPreview,
    files: Vec<RepairFile>,
}
struct RepairFile {
    path: WorkspacePath,
    expected_hash: Option<String>,
    bytes: Vec<u8>,
    id: Option<MemoryId>,
    title: String,
}

impl Vault {
    pub fn preview_repair(&self, actor: &WriteActor) -> MemoryResult<MemoryRepairPreview> {
        self.rebuild()?;
        let mut state = self.lock();
        let mut files = vec![];
        let mut changes = vec![];
        let mut manual = vec![];
        let mut total = 0;
        for (name, reason) in state.index.invalid_files()? {
            let path =
                WorkspacePath::parse(&name).map_err(|_| MemoryError::Invalid("indexed path"))?;
            if reason == "duplicate memory id" {
                manual.push(MemoryInvalidFile {
                    path: name,
                    reason: "重复 ID；保留两份文件，请手动决定".into(),
                });
                continue;
            }
            let read = match state.fs.read(&path) {
                Ok(r) => r,
                Err(_) => {
                    manual.push(MemoryInvalidFile {
                        path: name,
                        reason: "文件或路径不可安全读取；请手动处理".into(),
                    });
                    continue;
                }
            };
            let text = match std::str::from_utf8(&read.bytes) {
                Ok(s) => s,
                Err(_) => {
                    manual.push(MemoryInvalidFile {
                        path: name,
                        reason: "不是 UTF-8；请手动转换".into(),
                    });
                    continue;
                }
            };
            let category = name.split_once('/').map_or("experience", |(category, _)| category);
            let fixed = match repair_missing_fields(text, self.scope().scope(), category) {
                Ok(Some(fixed)) => fixed,
                Ok(None) if reason == "missing or ambiguous local relation" => {
                    let mut note = parse(text)?;
                    let mut ambiguous = false;
                    for ids in [
                        &mut note.metadata.related_ids,
                        &mut note.metadata.supersedes_ids,
                        &mut note.metadata.conflicts_with_ids,
                    ] {
                        let mut valid = vec![];
                        for id in ids.iter() {
                            match state.index.id_count(&id.to_string())? {
                                0 => {}
                                1 => valid.push(*id),
                                _ => {
                                    ambiguous = true;
                                    valid.push(*id);
                                }
                            }
                        }
                        *ids = valid;
                    }
                    if ambiguous
                        || note.metadata.status == MemoryStatus::Conflicted
                            && note.metadata.conflicts_with_ids.is_empty()
                    {
                        manual.push(MemoryInvalidFile {
                            path: name,
                            reason: "关联含重复 ID 或冲突对象已缺失；请手动决定".into(),
                        });
                        continue;
                    }
                    (note, vec!["移除不在此 Vault 中的关联；正文未改动".into()])
                }
                _ => {
                    manual.push(MemoryInvalidFile {
                        path: name,
                        reason: "现有字段或 YAML 结构需要手动修复；正文未改动".into(),
                    });
                    continue;
                }
            };
            let bytes = render(&fixed.0)?.into_bytes();
            if let Some(kind) = aura_secret_scanner::detect_text(
                std::str::from_utf8(&bytes).map_err(|_| MemoryError::Invalid("UTF-8"))?,
            ) {
                manual.push(MemoryInvalidFile {
                    path: name,
                    reason: format!("含密钥样式（{kind}）；请先手动处理"),
                });
                continue;
            }
            total += bytes.len();
            if files.len() >= MAX_BATCH || total > 8 * 1024 * 1024 {
                manual.push(MemoryInvalidFile {
                    path: name,
                    reason: "本次预览达到上限；请分批修复".into(),
                });
                continue;
            }
            changes.push(MemoryRepairChange {
                path: path.clone(),
                expected_hash: Some(read.hash.clone()),
                proposed_hash: blake3::hash(&bytes).to_hex().to_string(),
                changes: fixed.1,
            });
            files.push(RepairFile {
                path,
                expected_hash: Some(read.hash),
                bytes,
                id: Some(fixed.0.metadata.id),
                title: fixed.0.title(),
            });
        }
        // The same set-up rule as a first write, so a deliberately removed
        // .gitignore is never offered back.
        for (name, bytes) in crate::writer::layout_files(&state, self.scope().scope())? {
            let path = WorkspacePath::parse(name).expect("layout leaf");
            changes.push(MemoryRepairChange {
                path: path.clone(),
                expected_hash: None,
                proposed_hash: blake3::hash(bytes).to_hex().to_string(),
                changes: vec![format!("补建 {name}；不改正文")],
            });
            files.push(RepairFile {
                path,
                expected_hash: None,
                bytes: bytes.to_vec(),
                id: None,
                title: name.into(),
            });
        }
        let token = format!("repair_{}", MemoryId::new());
        let preview = MemoryRepairPreview {
            preview_token: token.clone(),
            changes,
            manual,
            rebuild_index: true,
        };
        state.previews.retain(|_, p| p.at.elapsed() < std::time::Duration::from_secs(600));
        if state.previews.len() >= 8 {
            state.previews.clear();
        }
        state.previews.insert(
            token,
            PendingRepair {
                owner: (actor.kind.clone(), actor.session_id),
                at: std::time::Instant::now(),
                preview: preview.clone(),
                files,
            },
        );
        Ok(preview)
    }
    pub fn apply_repair(
        &self,
        token: &str,
        actor: &WriteActor,
    ) -> MemoryResult<MemoryRepairPreview> {
        let mut state = self.lock();
        if state.recovery_required {
            return Err(MemoryError::RecoveryRequired);
        }
        let pending = state
            .previews
            .remove(token)
            .ok_or(MemoryError::Invalid("preview missing, used or expired; preview again"))?;
        if pending.owner != (actor.kind.clone(), actor.session_id)
            || pending.at.elapsed() > std::time::Duration::from_secs(600)
        {
            return Err(MemoryError::Invalid("preview owner or expiry"));
        }
        let changes = pending
            .files
            .iter()
            .map(|f| VaultChange {
                path: f.path.clone(),
                expected_hash: f.expected_hash.clone(),
                bytes: Some(f.bytes.clone()),
            })
            .collect::<Vec<_>>();
        let history = pending
            .files
            .iter()
            .map(|f| MemoryHistory {
                id: f.id,
                path: f.path.as_str().into(),
                action: "repair".into(),
                actor: actor.kind.clone(),
                session_id: actor.session_id,
                turn_id: actor.turn_id.clone(),
                before_hash: f.expected_hash.clone(),
                after_hash: Some(blake3::hash(&f.bytes).to_hex().to_string()),
                title: f.title.clone(),
                at: UtcTimestamp::now(),
            })
            .collect::<Vec<_>>();
        if !changes.is_empty() {
            if changes.iter().any(|c| c.path.as_str() == "README.md") {
                crate::writer::ensure_layout_directories(&state, self.scope().scope())?;
            }
            crate::journal::publish(&mut state, &changes, &history)?;
            crate::writer::refresh_cache(&mut state, &[], &[], &history);
        }
        drop(state);
        // The batch is committed, so the repair is done. A rebuild that fails here leaves the
        // index's own notice (verify, then rebuild) and does not make the repair's outcome unknown.
        let _ = self.rebuild();
        Ok(pending.preview)
    }
}
