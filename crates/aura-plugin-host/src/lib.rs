//! A plugin's background service: started once the user turned it on in the plugin page and the
//! plugin can be used, stopped otherwise, started again after it ended with a growing wait. It
//! talks to Aura only on its own standard input and output, one JSON object per line, in a
//! versioned protocol that knows no chat platform; its standard error goes to the plugin's log
//! with the secret values it was given taken out. It runs as the user in a Job of its own under
//! the plugin's limits. What its requests do is not decided here: the host hands each one to
//! [`ServiceCalls`].
mod backoff;
mod host;
mod instance;
mod launch;
mod lines;
mod protocol;
mod redact;

pub use backoff::{Backoff, FIRST_DELAY, MAX_DELAY, STABLE_AFTER};
pub use host::{
    CallFuture, Clock, MAX_IN_FLIGHT, PluginHost, READY_WITHIN, SHUTDOWN_GRACE, ServiceCalls,
    SystemClock,
};
pub use launch::{Launcher, ProgramLauncher, ServiceLaunch, ServiceProcess};
pub use lines::{ErrorLines, Line, LineReader};
pub use protocol::{
    ApprovalOffer, ChannelAbilities, ChatAddress, FromService, LineProblem, LogLevel,
    MAX_CHANNEL_TEXT_CHARS, MAX_LINE_BYTES, MAX_REQUEST_ID_CHARS, MenuCommand, PROTOCOL_VERSION,
    ReplyFormat, RequestId, ServiceError, ToService, parse_line, shown,
};
pub use redact::{Admit, HIDDEN, LOG_LINES_PER_MINUTE, LogGate, Redactor};
