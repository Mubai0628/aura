//! The picture-in-picture window of the user's child session: `aura-desktop-host.exe --pip
//! --size <w>x<h> --stop-keys <keys>`, started by Core in the user's own session.
//!
//! It says [`PipMessage::Ready`] first, then connects a Remote Desktop control to the child
//! session, where Windows asks the user to sign in; a child session from before is signed out
//! first, only when the user says so. While a turn keeps the child session's desktop the picture
//! passes no input, and a click into it, the stop shortcut or 停止 takes the desktop over. Closing
//! the window, after the user confirms, signs the child session out.
pub mod state;

use crate::lines::{Line, next_line};
use crate::native::rdp::RdpEvent;
use crate::native::session;
use crate::native::window::{self, Input, Poster, Ui};
use aura_desktop_domain::Chord;
use aura_desktop_domain::input::{VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT};
use aura_desktop_domain::pip::{
    PIP_PROTOCOL_VERSION, PipCommand, PipMessage, PipOptions, stop_chord,
};
use state::{Effect, Event, Pip};
use std::ffi::OsString;
use std::io::{BufReader, Write};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

const CLOSE_ASKS: &str = "关闭小窗会注销子会话，里面没保存的内容会丢失。";
const EXISTING_ASKS: &str =
    "子会话还开着，是上次留下的。重新打开要先注销它，里面没保存的内容会丢失。";
/// How long the window waits for Windows to report the child session's id after it signed in.
const ID_TICKS: u32 = 40;
/// The longest command line read.
const COMMAND_BYTES: usize = 4096;

type Out = Arc<Mutex<std::io::Stdout>>;

fn say(out: &Out, message: &PipMessage) {
    let mut out = out.lock().unwrap_or_else(PoisonError::into_inner);
    if let Ok(mut line) = serde_json::to_vec(message) {
        line.push(b'\n');
        let _ = out.write_all(&line).and_then(|()| out.flush());
    }
}

/// Runs the window with the arguments after `--pip`.
#[must_use]
pub fn run(arguments: &[OsString]) -> ExitCode {
    let Some(arguments) = arguments
        .iter()
        .map(|argument| argument.to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()
    else {
        return ExitCode::FAILURE;
    };
    let Ok(options) = PipOptions::parse(&arguments) else {
        return ExitCode::FAILURE;
    };
    let Ok(chord) = stop_chord(&options.stop_keys) else {
        return ExitCode::FAILURE;
    };
    if crate::native::ole().is_err() {
        return ExitCode::FAILURE;
    }
    let out: Out = Arc::new(Mutex::new(std::io::stdout()));
    say(&out, &PipMessage::Ready { protocol: PIP_PROTOCOL_VERSION });
    let mut app = App { pip: Pip::new(), options, chord, out: Arc::clone(&out), ticks: 0 };
    match window::run(move |ui, input| app.input(ui, input)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            say(&out, &PipMessage::Closed { signed_out: session::child_session().is_none() });
            ExitCode::FAILURE
        }
    }
}

struct App {
    pip: Pip,
    options: PipOptions,
    chord: Chord,
    out: Out,
    ticks: u32,
}

impl App {
    fn input(&mut self, ui: &mut Ui, input: Input) {
        let event = match input {
            Input::Created => {
                read_commands(ui.poster());
                let (modifiers, key) = hot_key(&self.chord);
                if !ui.register_stop_keys(modifiers, key) {
                    say(
                        &self.out,
                        &PipMessage::StopKeysTaken { keys: self.options.stop_keys.clone() },
                    );
                }
                let configured =
                    ui.rdp().map(|rdp| rdp.configure(self.options.width, self.options.height));
                if !matches!(configured, Some(Ok(()))) {
                    Event::CoreGone
                } else {
                    Event::Start { existing: session::child_session() }
                }
            }
            Input::Rdp(RdpEvent::SigningIn) => Event::SigningIn,
            Input::Rdp(RdpEvent::LoginComplete) => {
                self.ticks = 0;
                ui.tick(true);
                Event::SignedIn { session: session::child_session() }
            }
            Input::Rdp(RdpEvent::Disconnected(reason) | RdpEvent::FatalError(reason)) => {
                ui.tick(false);
                Event::Disconnected { reason }
            }
            Input::Rdp(RdpEvent::Connecting | RdpEvent::Connected) => return,
            Input::Command(PipCommand::StopKeys { keys }) => {
                if let Ok(chord) = stop_chord(&keys) {
                    let (modifiers, key) = hot_key(&chord);
                    if ui.register_stop_keys(modifiers, key) {
                        self.chord = chord;
                        self.options.stop_keys = keys;
                        say(&self.out, &PipMessage::StopKeysReady);
                    } else {
                        ui.set_status("停止快捷键已被占用");
                        say(&self.out, &PipMessage::StopKeysTaken { keys });
                    }
                }
                return;
            }
            Input::Command(command) => Event::Command(command),
            Input::CommandsEnded => Event::CoreGone,
            Input::Clicked => Event::ClickedIn,
            Input::StopKeys => Event::StopKeys,
            Input::StopButton => Event::StopButton,
            Input::ReconnectButton => Event::ReconnectButton,
            Input::CloseAsked => {
                self.feed(ui, Event::StopButton);
                if !ui.confirm(CLOSE_ASKS) {
                    return;
                }
                Event::CloseConfirmed
            }
            Input::Tick => {
                self.ticks += 1;
                if self.pip.signed_in() || self.ticks > ID_TICKS {
                    ui.tick(false);
                    return;
                }
                Event::SignedIn { session: session::child_session() }
            }
        };
        self.feed(ui, event);
    }

    fn feed(&mut self, ui: &mut Ui, event: Event) {
        for effect in self.pip.handle(event) {
            match effect {
                Effect::Say(message) => say(&self.out, &message),
                Effect::Status(status) => ui.set_status(status.text()),
                Effect::PassInput(on) => {
                    if !ui.pass_input(on) {
                        self.feed(ui, Event::ProtectionFailed);
                        return;
                    }
                }
                Effect::Watch(on) => {
                    if !ui.watch(on) {
                        self.feed(ui, Event::ProtectionFailed);
                        return;
                    }
                    ui.enable_stop(on);
                }
                Effect::ShowReconnect(on) => ui.show_reconnect(on),
                Effect::AskSignOutExisting(_) => {
                    let sign_out = ui.confirm(EXISTING_ASKS);
                    self.feed(ui, Event::ExistingAnswered { sign_out });
                }
                Effect::SignOut(id) => {
                    self.feed(ui, Event::ExistingSignedOut { success: session::sign_out(id) });
                }
                Effect::Connect => {
                    if ui.rdp().is_none_or(|rdp| rdp.connect().is_err()) {
                        self.feed(ui, Event::Disconnected { reason: -1 });
                    }
                }
                Effect::SignOutAndClose => {
                    if let Some(rdp) = ui.rdp() {
                        rdp.disconnect();
                    }
                    if let Some(child) = session::child_session() {
                        let _ = session::sign_out(child);
                    }
                    say(
                        &self.out,
                        &PipMessage::Closed { signed_out: session::child_session().is_none() },
                    );
                    ui.close();
                }
                Effect::CloseKeeping => {
                    if let Some(rdp) = ui.rdp() {
                        rdp.disconnect();
                    }
                    say(
                        &self.out,
                        &PipMessage::Closed { signed_out: session::child_session().is_none() },
                    );
                    ui.close();
                }
            }
        }
    }
}

/// Reads Core's commands from standard input on a thread of their own, and hands each to the
/// window; the end of the input too.
fn read_commands(poster: Poster) {
    std::thread::spawn(move || {
        let mut input = BufReader::new(std::io::stdin());
        while let Ok(Some(line)) = next_line(&mut input, COMMAND_BYTES) {
            let Line::Line(line) = line else { break };
            let Ok(command) = serde_json::from_slice::<PipCommand>(&line) else { break };
            poster.command(command);
        }
        poster.commands_ended();
    });
}

fn hot_key(chord: &Chord) -> (HOT_KEY_MODIFIERS, u32) {
    let mut modifiers = MOD_NOREPEAT;
    for key in &chord.modifiers {
        modifiers |= match key.code {
            VK_CONTROL => MOD_CONTROL,
            VK_MENU => MOD_ALT,
            VK_SHIFT => MOD_SHIFT,
            VK_LWIN => MOD_WIN,
            _ => HOT_KEY_MODIFIERS(0),
        };
    }
    (modifiers, u32::from(chord.key.code))
}
