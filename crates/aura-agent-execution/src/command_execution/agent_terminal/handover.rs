//! A user's own terminal the user handed to the model with @. From the handover on it is kept as
//! the model's terminals are: its output is saved for the conversation, and the model's terminal
//! tools act on it while the model controls it. It stays the user's: the model cannot close it,
//! the user takes it back with the takeover, and what it printed before the handover is not saved
//! (the message's attachment carries what the user chose of it).
use super::*;
use aura_backend_windows_user::compat::TerminalKind;

/// What the model is told when it tries to close a terminal the user handed over.
pub(super) const USER_OWNED: &str = "terminal.user_owned: 这是用户自己的终端，用户把它交给你使用，模型不能关闭它；用完让它开着，用户会自己关。";

impl AgentTerminals {
    /// Gives the terminal to the model. A user's terminal is kept from now on; one handed over
    /// before, or the model's own, goes back to the model's control.
    pub async fn hand_over(
        self: &Arc<Self>,
        session: SessionId,
        id: String,
    ) -> Result<TerminalViewV1> {
        let reader = self.clone();
        let target = id.clone();
        let known = self
            .store
            .run_critical(
                StoreStep::operation("terminal.user.hand_over"),
                StoreAccess::Read,
                move || Ok(reader.record(session, &target).ok()),
            )
            .await?;
        if let Some(record) = known {
            ensure!(
                record.view.end.is_none(),
                "terminal.ended: 终端已结束，没有交给模型。请新建终端。"
            );
            return self
                .control(TerminalControlRequestV1 {
                    session_id: session,
                    terminal_id: id,
                    controller: TerminalControllerV1::Agent,
                })
                .await;
        }
        let owner = self.clone();
        let view = self
            .store
            .run_critical(
                StoreStep::operation("terminal.user.hand_over"),
                StoreAccess::Write,
                move || {
                    // Handed over meanwhile: the terminal's one record stands.
                    if let Ok(record) = owner.record(session, &id) {
                        return Ok(record.view);
                    }
                    let summary = owner
                        .registry
                        .list(session)?
                        .into_iter()
                        .find(|entry| entry.id == id && entry.kind == TerminalKind::User)
                        .context("terminal.not_found_in_session")?;
                    let terminal = owner.registry.get(session, &id)?;
                    ensure!(
                        terminal.end()?.is_none(),
                        "terminal.ended: 终端已结束，没有交给模型。请新建终端。"
                    );
                    let record = AgentTerminalRecord {
                        view: TerminalViewV1 {
                            session_id: session,
                            terminal_id: id,
                            shell: summary.shell,
                            process_id: summary.process_id,
                            started_at: summary.started_at,
                            end: None,
                            owner: TerminalOwnerV1::User,
                            controller: TerminalControllerV1::Agent,
                            title: String::new(),
                            tool_call_id: None,
                            // Past the user's own view of it, which never changes revision, so
                            // a list read before the handover cannot show it as before.
                            revision: Revision::new(Revision::INITIAL.value() + 1),
                            handed_over: true,
                        },
                        // Not an operation of the model's: nothing stops it as one.
                        operation_id: SecurityOperationId::new(),
                        runtime_instance_id: owner.runtime,
                        working_directory: String::new(),
                        requires_network: false,
                        tab_closed: false,
                    };
                    let from = terminal.output_end()?;
                    owner.register(record.clone(), terminal, from)?;
                    Ok(record.view)
                },
            )
            .await?;
        self.publish(&view).await?;
        Ok(view)
    }

    /// Whether the terminal is the user's own, handed over.
    pub(super) fn users(&self, session: SessionId, id: &str) -> Result<bool> {
        Ok(self.record(session, id)?.view.owner == TerminalOwnerV1::User)
    }
}
