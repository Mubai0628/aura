//! Each finished maintenance run as one line of plain text beside the Vault's records, for a person
//! to read in Aura or in any editor. The run files stay the record; the log only
//! repeats them, so a log that cannot be written never fails the run it describes.
use crate::{MemoryMaintenanceRun, MemoryMaintenanceState};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const LOG: &str = "maintenance.log";
/// The previous part, replaced each time the log starts over.
const PREVIOUS: &str = "maintenance.1.log";
const ROTATE_AT: u64 = 1024 * 1024;

pub(crate) fn path(records: &Path) -> PathBuf {
    records.join(LOG)
}

pub(crate) fn finished(state: MemoryMaintenanceState) -> bool {
    !matches!(state, MemoryMaintenanceState::Queued | MemoryMaintenanceState::Running)
}

pub(crate) fn append(records: &Path, run: &MemoryMaintenanceRun) -> std::io::Result<()> {
    let log = path(records);
    if fs::metadata(&log).is_ok_and(|meta| meta.len() >= ROTATE_AT) {
        fs::rename(&log, records.join(PREVIOUS))?;
    }
    OpenOptions::new().create(true).append(true).open(&log)?.write_all(line(run).as_bytes())
}

fn line(run: &MemoryMaintenanceRun) -> String {
    let state = match run.state {
        MemoryMaintenanceState::Queued => "等待",
        MemoryMaintenanceState::Running => "进行中",
        MemoryMaintenanceState::Completed => "已完成",
        MemoryMaintenanceState::Skipped => "已跳过",
        MemoryMaintenanceState::Failed => "失败",
        MemoryMaintenanceState::Interrupted => "已中断",
    };
    let reason = run
        .reason
        .as_deref()
        .map(|reason| reason.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|reason| !reason.is_empty())
        .map(|reason| format!("：{reason}"))
        .unwrap_or_default();
    format!(
        "{}  {state}{reason}  会话 {}  用量 输入 {} · 输出 {} token{}\n",
        run.finished_at.unwrap_or(run.started_at),
        run.session_id,
        run.input_tokens,
        run.output_tokens,
        if run.usage_complete { "" } else { "（用量未确认）" },
    )
}
