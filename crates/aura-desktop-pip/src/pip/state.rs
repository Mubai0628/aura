//! What the picture-in-picture window does on each thing that happens, apart from Windows: the
//! window's native side reports events and carries out the effects in order.
use aura_desktop_domain::pip::{PipCommand, PipMessage};

/// What the status line shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Connecting,
    SigningIn,
    Idle,
    Controlling,
    TakenOver,
    Disconnected,
}

impl Status {
    #[must_use]
    pub fn text(self) -> &'static str {
        match self {
            Self::Connecting => "正在连接",
            Self::SigningIn => "等待登录",
            Self::Idle => "空闲",
            Self::Controlling => "正在操控小窗",
            Self::TakenOver => "你已接管小窗",
            Self::Disconnected => "已断开",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    /// The window is up; a child session from before exists or not.
    Start {
        existing: Option<u32>,
    },
    /// The user answered whether to sign the child session from before out.
    ExistingAnswered {
        sign_out: bool,
    },
    ExistingSignedOut {
        success: bool,
    },
    ProtectionFailed,
    Command(PipCommand),
    /// Windows shows its sign-in prompt.
    SigningIn,
    /// The child session is signed in; its id once Windows reports it.
    SignedIn {
        session: Option<u32>,
    },
    Disconnected {
        reason: i32,
    },
    /// A click of the user's into the child session's picture.
    ClickedIn,
    StopKeys,
    StopButton,
    ReconnectButton,
    /// The user closed the window and confirmed signing the child session out.
    CloseConfirmed,
    /// Core closed the window's input without telling it to close: the window goes, the child
    /// session stays, and the next opening asks before signing it out.
    CoreGone,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    Say(PipMessage),
    Status(Status),
    /// Whether the picture passes the user's input to the child session.
    PassInput(bool),
    /// Whether 停止 can be pressed, and the clicks into the picture are watched.
    Watch(bool),
    ShowReconnect(bool),
    AskSignOutExisting(u32),
    SignOut(u32),
    Connect,
    /// Disconnects, signs the child session out, says [`PipMessage::Closed`] and exits.
    SignOutAndClose,
    /// Disconnects, says [`PipMessage::Closed`] without signing anything out, and exits.
    CloseKeeping,
}

#[derive(Debug, Default)]
pub struct Pip {
    existing: Option<u32>,
    signed_in: Option<u32>,
    connected: bool,
    hold: bool,
    closing: bool,
}

impl Pip {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a turn keeps the child session's desktop now.
    #[must_use]
    pub fn holding(&self) -> bool {
        self.hold
    }

    /// Whether the child session is signed in and Windows reported its id.
    #[must_use]
    pub fn signed_in(&self) -> bool {
        self.signed_in.is_some()
    }

    pub fn handle(&mut self, event: Event) -> Vec<Effect> {
        if self.closing {
            return Vec::new();
        }
        match event {
            Event::Start { existing: Some(session) } => {
                self.existing = Some(session);
                vec![Effect::AskSignOutExisting(session)]
            }
            Event::Start { existing: None } => self.connect(),
            Event::ExistingAnswered { sign_out: false } => {
                self.closing = true;
                vec![Effect::CloseKeeping]
            }
            Event::ExistingAnswered { sign_out: true } => {
                self.existing.map(Effect::SignOut).into_iter().collect()
            }
            Event::ExistingSignedOut { success: true } if self.existing.take().is_some() => {
                self.connect()
            }
            Event::ExistingSignedOut { success: false } | Event::ProtectionFailed => {
                self.closing = true;
                self.hold = false;
                vec![
                    Effect::Say(PipMessage::Failed {
                        message: "小窗未能准备好安全的连接或输入接管，请关闭后重试。".to_owned(),
                    }),
                    Effect::Watch(false),
                    Effect::CloseKeeping,
                ]
            }
            Event::ExistingSignedOut { .. } => Vec::new(),
            Event::SigningIn if self.connected => vec![Effect::Status(Status::SigningIn)],
            Event::SigningIn => Vec::new(),
            Event::SignedIn { session: None } => Vec::new(),
            Event::SignedIn { session: Some(session) } => {
                if !self.connected
                    || session == 0
                    || session == u32::MAX
                    || self.signed_in == Some(session)
                {
                    return Vec::new();
                }
                self.signed_in = Some(session);
                let mut effects = vec![Effect::Say(PipMessage::SignedIn { session })];
                effects.extend(self.shown());
                effects
            }
            Event::Disconnected { reason } => {
                self.connected = false;
                self.signed_in = None;
                self.hold = false;
                vec![
                    Effect::Say(PipMessage::Disconnected { reason }),
                    Effect::PassInput(false),
                    Effect::Watch(false),
                    Effect::Status(Status::Disconnected),
                    Effect::ShowReconnect(true),
                ]
            }
            Event::ReconnectButton if !self.connected => self.connect(),
            Event::ReconnectButton => Vec::new(),
            Event::Command(PipCommand::Hold { on, request }) => {
                if self.hold == on {
                    return if self.signed_in.is_some() {
                        vec![Effect::Say(PipMessage::Holding { on, request })]
                    } else {
                        Vec::new()
                    };
                }
                self.hold = on;
                if self.signed_in.is_none() {
                    return Vec::new();
                }
                let mut effects = self.shown();
                effects.push(Effect::Say(PipMessage::Holding { on, request }));
                effects
            }
            Event::ClickedIn | Event::StopKeys | Event::StopButton => {
                if !self.hold || self.signed_in.is_none() {
                    return Vec::new();
                }
                self.hold = false;
                vec![
                    Effect::Say(PipMessage::TakenOver),
                    Effect::Watch(false),
                    Effect::PassInput(true),
                    Effect::Status(Status::TakenOver),
                ]
            }
            Event::Command(PipCommand::Close) | Event::CloseConfirmed => {
                self.closing = true;
                vec![Effect::Watch(false), Effect::SignOutAndClose]
            }
            Event::Command(PipCommand::StopKeys { .. }) => Vec::new(),
            Event::CoreGone => {
                self.closing = true;
                vec![Effect::Watch(false), Effect::CloseKeeping]
            }
        }
    }

    fn connect(&mut self) -> Vec<Effect> {
        self.connected = true;
        vec![
            Effect::ShowReconnect(false),
            Effect::Status(Status::Connecting),
            Effect::Say(PipMessage::Connecting),
            Effect::Connect,
        ]
    }

    /// The window as a signed-in child session shows it, with a hold or without.
    fn shown(&self) -> Vec<Effect> {
        if self.hold {
            vec![Effect::PassInput(false), Effect::Watch(true), Effect::Status(Status::Controlling)]
        } else {
            vec![Effect::Watch(false), Effect::PassInput(true), Effect::Status(Status::Idle)]
        }
    }
}
