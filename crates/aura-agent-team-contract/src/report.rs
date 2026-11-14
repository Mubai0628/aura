//! What a sub-agent hands back: progress or a question while it works, through `agent.report`,
//! and its final answer, which Aura keeps as its report. Reports kept before carry the statuses
//! and lists a sub-agent once wrote itself.
use crate::Refusal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const SUMMARY_MAX_CHARS: usize = 8_000;
const QUESTION_MAX_CHARS: usize = 1_000;
const LIST_MAX: usize = 50;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "AgentTeamReportStatus")]
pub enum ReportStatus {
    /// Still working; this is a note on the way.
    Progress,
    /// Stops until the main agent answers its one question.
    Waiting,
    /// Done: the summary holds what the main agent needs.
    Ready,
    /// Cannot go on without help.
    Blocked,
    /// Did not get it done.
    Failed,
}

impl ReportStatus {
    /// Whether the sub-agent stops after this report until the main agent sends it more.
    #[must_use]
    pub fn final_report(self) -> bool {
        self != Self::Progress
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Waiting => "waiting",
            Self::Ready => "ready",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
        }
    }
}

/// What an `agent.report` call takes: a note on the way, or one question for the main agent. Being
/// done, stuck or unable is said in the turn's final answer instead.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "AgentTeamReportInputStatus")]
pub enum ReportInputStatus {
    /// Still working; this is a note on the way.
    Progress,
    /// Stops until the main agent answers its one question.
    Waiting,
}

/// One `agent.report` call.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReportInput {
    pub status: ReportInputStatus,
    /// What was found or done so far, at most 8000 characters.
    pub summary: String,
    /// The one question for the main agent; required with waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
}

impl ReportInput {
    /// The report this call hands in.
    #[must_use]
    pub fn into_report(self) -> Report {
        let status = match self.status {
            ReportInputStatus::Progress => ReportStatus::Progress,
            ReportInputStatus::Waiting => ReportStatus::Waiting,
        };
        Report {
            status,
            summary: self.summary,
            question: self.question,
            files: Vec::new(),
            checks: Vec::new(),
            remaining: None,
        }
    }
}

/// One report, as the sub-agent wrote it.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub status: ReportStatus,
    /// What was found or done, at most 8000 characters. For ready, the result the main agent
    /// needs; long content goes in files or the final answer.
    pub summary: String,
    /// One question for the main agent: required with waiting, allowed with blocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// Files that matter for the result, relative to the workspace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Commands run to check the result, each with its exit code.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
    /// What is not done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<String>,
}

impl Report {
    pub fn check(&self) -> Result<(), Refusal> {
        let invalid = |message: &str| Err(Refusal::new("agent.report_invalid", message));
        if self.summary.trim().is_empty() {
            return invalid("summary 是空的：写上找到了什么、做了什么。");
        }
        let length = self.summary.chars().count();
        if length > SUMMARY_MAX_CHARS {
            return invalid(&format!(
                "summary 有 {length} 字，最多 8000 字：只写要点；代码等长内容写进文件（有工作副本时写在副本里），\
                 或放进这一轮的最终回答，主 Agent 能完整读到。"
            ));
        }
        match (&self.question, self.status) {
            (None, ReportStatus::Waiting) => return invalid("waiting 要带一个问题（question）。"),
            (Some(_), ReportStatus::Progress | ReportStatus::Ready | ReportStatus::Failed) => {
                return invalid("只有 waiting 能带问题。");
            }
            (Some(question), _)
                if question.trim().is_empty() || question.chars().count() > QUESTION_MAX_CHARS =>
            {
                return invalid("问题不能为空，最多 1000 字。");
            }
            _ => {}
        }
        if self.files.len() > LIST_MAX || self.checks.len() > LIST_MAX {
            return invalid("files 和 checks 各最多 50 条。");
        }
        if self.remaining.as_deref().is_some_and(|r| r.chars().count() > SUMMARY_MAX_CHARS) {
            return invalid("remaining 最多 8000 字。");
        }
        Ok(())
    }
}
