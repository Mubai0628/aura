use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The lifecycle points a Hook can subscribe to (detailed design §24.4).
///
/// `session.ended` waits for permanent session deletion, which V2 does not have yet, and
/// `model.stream_batch` is for declarative Hooks, which the first version does not have; a
/// manifest naming either is refused with that reason rather than silently never called.
#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
pub enum HookEvent {
    #[serde(rename = "session.started")]
    SessionStarted,
    #[serde(rename = "session.resumed")]
    SessionResumed,
    #[serde(rename = "message.before_accept")]
    MessageBeforeAccept,
    #[serde(rename = "turn.before_start")]
    TurnBeforeStart,
    #[serde(rename = "context.before_build")]
    ContextBeforeBuild,
    #[serde(rename = "model.before_call")]
    ModelBeforeCall,
    #[serde(rename = "model.after_call")]
    ModelAfterCall,
    #[serde(rename = "tool.before_call")]
    ToolBeforeCall,
    #[serde(rename = "tool.after_call")]
    ToolAfterCall,
    #[serde(rename = "tool.on_error")]
    ToolOnError,
    #[serde(rename = "context.before_compact")]
    ContextBeforeCompact,
    #[serde(rename = "context.after_compact")]
    ContextAfterCompact,
    #[serde(rename = "turn.before_complete")]
    TurnBeforeComplete,
    #[serde(rename = "turn.completed")]
    TurnCompleted,
}

impl HookEvent {
    pub const ALL: [Self; 14] = [
        Self::SessionStarted,
        Self::SessionResumed,
        Self::MessageBeforeAccept,
        Self::TurnBeforeStart,
        Self::ContextBeforeBuild,
        Self::ModelBeforeCall,
        Self::ModelAfterCall,
        Self::ToolBeforeCall,
        Self::ToolAfterCall,
        Self::ToolOnError,
        Self::ContextBeforeCompact,
        Self::ContextAfterCompact,
        Self::TurnBeforeComplete,
        Self::TurnCompleted,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::SessionStarted => "session.started",
            Self::SessionResumed => "session.resumed",
            Self::MessageBeforeAccept => "message.before_accept",
            Self::TurnBeforeStart => "turn.before_start",
            Self::ContextBeforeBuild => "context.before_build",
            Self::ModelBeforeCall => "model.before_call",
            Self::ModelAfterCall => "model.after_call",
            Self::ToolBeforeCall => "tool.before_call",
            Self::ToolAfterCall => "tool.after_call",
            Self::ToolOnError => "tool.on_error",
            Self::ContextBeforeCompact => "context.before_compact",
            Self::ContextAfterCompact => "context.after_compact",
            Self::TurnBeforeComplete => "turn.before_complete",
            Self::TurnCompleted => "turn.completed",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.name() == name)
    }

    /// Events whose block stops the action that follows. On every other event a block (or
    /// a fail-closed failure) only stops the later Hooks of the same event; nothing that
    /// already happened is rolled back.
    pub fn blocks_action(self) -> bool {
        matches!(
            self,
            Self::MessageBeforeAccept
                | Self::TurnBeforeStart
                | Self::ContextBeforeBuild
                | Self::ModelBeforeCall
                | Self::ToolBeforeCall
                | Self::TurnBeforeComplete
        )
    }

    /// The one result field a `transform` decision may carry on this event.
    pub fn transform_field(self) -> Option<&'static str> {
        match self {
            Self::MessageBeforeAccept | Self::ModelAfterCall => Some("text"),
            Self::ContextBeforeBuild => Some("inject"),
            Self::ModelBeforeCall => Some("parameters"),
            Self::ToolBeforeCall => Some("arguments"),
            Self::ToolAfterCall => Some("result"),
            Self::ToolOnError => Some("error"),
            Self::ContextBeforeCompact => Some("focus"),
            Self::SessionStarted
            | Self::SessionResumed
            | Self::TurnBeforeStart
            | Self::ContextAfterCompact
            | Self::TurnBeforeComplete
            | Self::TurnCompleted => None,
        }
    }

    pub fn tool_scoped(self) -> bool {
        matches!(self, Self::ToolBeforeCall | Self::ToolAfterCall | Self::ToolOnError)
    }
}

/// Events the design names that the first version cannot call, with the reason.
pub(crate) fn unavailable(name: &str) -> Option<&'static str> {
    match name {
        "session.ended" => {
            Some("session.ended 只在永久删除会话前触发，Aura 还没有永久删除会话的功能")
        }
        "model.stream_batch" => Some("model.stream_batch 只给声明式 Hook，首版没有声明式 Hook"),
        _ => None,
    }
}
