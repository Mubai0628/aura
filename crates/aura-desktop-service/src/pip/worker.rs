use super::{Opened, State, lock};
use crate::{HostEvent, child::ChildHost};
use aura_desktop_domain::{
    PipPhase,
    pip::{PIP_PROTOCOL_VERSION, PipCommand, PipMessage},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::{mpsc, watch};

struct Running(Opened);
fn update(
    shared: &Mutex<State>,
    changed: &watch::Sender<u64>,
    generation: u64,
    work: impl FnOnce(&mut State),
) {
    let mut state = lock(shared);
    if state.generation == generation {
        work(&mut state);
        changed.send_modify(|revision| *revision += 1);
    }
}
pub(super) fn spawn(
    opened: Opened,
    mut commands: mpsc::UnboundedReceiver<PipCommand>,
    shared: Arc<Mutex<State>>,
    changed: watch::Sender<u64>,
    child: ChildHost,
    generation: u64,
    within: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut running = Running(opened);
        let mut greeted = false;
        let mut signed_in = None;
        let mut authenticated = None;
        let mut published = false;
        let deadline = tokio::time::Instant::now() + within;
        let reason = loop {
            tokio::select! {
                result = &mut running.0.login, if authenticated.is_none() => {
                    match result { Ok(id) => authenticated = Some(id), Err(why) => break why }
                }
                _ = tokio::time::sleep_until(deadline), if signed_in.is_none() || authenticated.is_none() => break "小窗没有登录，等待已超时；没有执行".to_owned(),
                command = commands.recv() => {
                    let Some(command) = command else { break "小窗连接已结束".to_owned() };
                    let mut line = serde_json::to_vec(&command).unwrap_or_default(); line.push(b'\n');
                    if running.0.window.send(line).is_err() { break "小窗已关闭；没有执行".to_owned(); }
                }
                event = running.0.events.recv() => {
                    let Some(HostEvent::Line(line)) = event else { break "小窗已关闭或返回了无效消息；没有执行".to_owned() };
                    let Ok(message) = serde_json::from_slice::<PipMessage>(&line) else { break "小窗的消息无法识别；没有执行".to_owned() };
                    if !greeted {
                        if message != (PipMessage::Ready { protocol: PIP_PROTOCOL_VERSION }) { break "小窗协议不匹配；没有执行".to_owned(); }
                        greeted = true;
                        continue;
                    }
                    match message {
                        PipMessage::SignedIn { session } => signed_in = Some(session),
                        PipMessage::Holding { on, request } => update(&shared, &changed, generation, |state| { if request == state.request { state.held = on && state.owner.is_some(); state.acknowledgement = request; } }),
                        PipMessage::TakenOver => {
                            update(&shared, &changed, generation, |state| { if let Some(owner) = state.owner.take() { state.blocked.insert(owner); } state.held = false; });
                            child.interrupt();
                        }
                        PipMessage::StopKeysTaken { keys } => update(&shared, &changed, generation, |state| state.status.notice = Some(format!("停止快捷键 {keys} 已被占用，请修改快捷键；仍可用小窗的停止按钮接管。"))),
                        PipMessage::StopKeysReady => update(&shared, &changed, generation, |state| state.status.notice = None),
                        PipMessage::Closed { signed_out } => break if signed_out { "小窗已关闭，子会话已注销；没有执行".to_owned() } else { "小窗已关闭，子会话未注销；重新打开时请确认处理未保存内容。".to_owned() },
                        PipMessage::Disconnected { .. } => break "小窗已断开，请重新打开并登录；没有执行".to_owned(),
                        PipMessage::Failed { message } => break message,
                        PipMessage::Ready { .. } => break "小窗重复初始化；没有执行".to_owned(),
                        PipMessage::Connecting => {},
                    }
                }
            }
            if let (Some(display), Some(verified), false) = (signed_in, authenticated, published) {
                if display != verified {
                    break "小窗和操控主机不在同一子会话；没有执行".to_owned();
                }
                if let Err(why) = running.0.startup.remove() {
                    break why;
                }
                update(&shared, &changed, generation, |state| state.status.phase = PipPhase::Ready);
                published = true;
            }
        };
        child.close();
        update(&shared, &changed, generation, |state| {
            state.owner = None;
            state.held = false;
            state.status.phase = PipPhase::Failed;
            state.status.notice = Some(reason);
            state.commands = None;
        });
    })
}
