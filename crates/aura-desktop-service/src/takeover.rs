//! Taking the desktop over and giving it back. Once the user takes the desktop over from a turn,
//! the turn may only read until the user gives the desktop back or the turn ends. The host refuses
//! a hold it ended for a takeover, so a turn given the desktop back acts again in a new hold. A
//! turn that ended taken over stays listed until the user gives the desktop back or the session's
//! next turn runs.
use super::*;

/// A turn that ended while the user had the desktop, with the program of its latest action.
pub(super) struct Ended {
    turn: TurnTaskId,
    app_id: String,
    name: String,
    foreground: bool,
}

/// The turn's hold now: its first one, or the one after the latest give-back.
pub(super) fn key_in(state: &State, turn: TurnTaskId) -> String {
    match state.generations.get(&turn) {
        Some(generation) => format!("{turn}#{generation}"),
        None => turn.to_string(),
    }
}

/// Adds `row` to the list, or what it says to the row of the same program.
pub(super) fn list(shown: &mut Vec<Controlled>, row: Controlled) {
    match shown.iter_mut().find(|shown| shown.app_id == row.app_id) {
        Some(shown) => {
            shown.foreground |= row.foreground;
            shown.taken_over |= row.taken_over;
        }
        None => shown.push(row),
    }
}

/// The session's turn that ended taken over, as the activity list shows it.
pub(super) fn ended_row(
    state: &State,
    session: SessionId,
    mode: aura_desktop_domain::ComputerMode,
) -> Option<Controlled> {
    state.ended.get(&session).map(|ended| Controlled {
        app_id: ended.app_id.clone(),
        name: ended.name.clone(),
        foreground: ended.foreground,
        taken_over: true,
        mode,
    })
}

impl DesktopControl {
    pub(super) fn key(&self, turn: TurnTaskId) -> String {
        key_in(&self.lock(), turn)
    }

    /// Whether the user took this desktop, not its child's, over from the turn in hold `key`.
    fn taken_here(&self, session: SessionId, turn: TurnTaskId, key: &str) -> bool {
        let marked = self.lock().taken_over.contains(&(session, turn));
        marked
            || self.link.hold_end(key) == Some(HoldEnd::TakenOver)
            || self.pip.as_ref().is_some_and(|pip| pip.taken_over(session, turn))
    }

    /// Whether the user took the desktop over from the turn: its actions are refused until the
    /// user gives the desktop back.
    #[must_use]
    pub fn taken_over(&self, session: SessionId, turn: TurnTaskId) -> bool {
        self.taken_here(session, turn, &self.key(turn))
            || self.child.get().is_some_and(|child| child.taken_over(session, turn))
    }

    /// What of the user's took the desktop over from the turn, when the host said.
    #[must_use]
    pub fn takeover_cause(&self, turn: TurnTaskId) -> Option<String> {
        self.link
            .takeover_cause(&self.key(turn))
            .or_else(|| self.child.get().and_then(|child| child.takeover_cause(turn)))
    }

    /// The turn waits for the user, to approve or to answer: the desktop it kept is given back
    /// meanwhile, since the user needs the mouse for that. Its next action keeps it again.
    pub async fn turn_waits(&self, turn: TurnTaskId) {
        self.link.release(&self.key(turn)).await;
    }

    /// A turn of the session runs: the session's earlier turn that ended taken over is no longer
    /// listed, since this one may act.
    pub fn turn_runs(&self, session: SessionId, turn: TurnTaskId) {
        {
            let mut state = self.lock();
            state.latest.insert(session, turn);
            if state.ended.get(&session).is_some_and(|ended| ended.turn != turn) {
                state.ended.remove(&session);
            }
        }
        if let Some(child) = self.child.get() {
            child.turn_runs(session, turn);
        }
    }

    /// The user gives the desktop back to the session: each of its turns the user took the
    /// desktop over from may act again, in a new hold, and its turn that ended taken over is no
    /// longer listed.
    pub async fn give_back(&self, session: SessionId) {
        self.give_back_one(session).await;
        if let Some(child) = self.child.get() {
            child.give_back_one(session).await;
        }
    }

    async fn give_back_one(&self, session: SessionId) {
        let (mut turns, holds) = {
            let state = self.lock();
            let turns: BTreeSet<TurnTaskId> = state
                .taken_over
                .iter()
                .filter(|(owner, _)| *owner == session)
                .map(|(_, turn)| *turn)
                .collect();
            let holds: Vec<(String, TurnTaskId)> = state
                .holds
                .iter()
                .filter(|(_, hold)| hold.session == session)
                .map(|(key, hold)| (key.clone(), hold.turn))
                .collect();
            (turns, holds)
        };
        for (key, turn) in holds {
            if self.taken_here(session, turn, &key) {
                turns.insert(turn);
            }
        }
        let mut released = Vec::new();
        {
            let mut state = self.lock();
            state.ended.remove(&session);
            for turn in &turns {
                state.taken_over.remove(&(session, *turn));
                let key = key_in(&state, *turn);
                state.holds.remove(&key);
                *state.generations.entry(*turn).or_default() += 1;
                released.push(key);
            }
        }
        // A takeover in the child desktop's window leaves the hold kept by its host.
        for key in released {
            self.link.release(&key).await;
        }
        if let Some(pip) = &self.pip {
            for turn in turns {
                pip.finished(session, turn);
            }
        }
    }

    /// The turn ended: the desktop it kept is given back. When the user had taken it over, the
    /// turn stays listed, unless the session's next turn already runs.
    pub(super) async fn turn_ended_one(&self, session: SessionId, turn: TurnTaskId) {
        let key = self.key(turn);
        let taken = self.taken_here(session, turn, &key);
        {
            let mut state = self.lock();
            let hold = state.holds.remove(&key);
            state.taken_over.remove(&(session, turn));
            state.generations.remove(&turn);
            let last = match state.latest.get(&session) {
                Some(latest) if *latest != turn => false,
                Some(_) => {
                    state.latest.remove(&session);
                    true
                }
                None => true,
            };
            if let (true, true, Some(hold)) = (taken, last, hold) {
                state.ended.insert(
                    session,
                    Ended {
                        turn,
                        app_id: hold.app_id,
                        name: hold.name,
                        foreground: hold.foreground,
                    },
                );
            }
        }
        self.link.release(&key).await;
        if let Some(pip) = &self.pip {
            pip.release(session, Some(turn)).await;
            pip.finished(session, turn);
        }
    }
}
