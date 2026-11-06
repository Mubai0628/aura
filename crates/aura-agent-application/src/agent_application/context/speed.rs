//! The speed mode a conversation chose for a turn. A turn taking in queued messages starts with
//! the newest new-turn message's own choice; each step of the turn sends it when the model's
//! settings allow speed modes and its catalog entry offers that one.
use aura_agent_domain::{AgentInputLane, AgentPendingMessage, TurnSpeedMode};
use aura_model_domain::{ModelProfileConfig, ModelSpeedMode};

/// The mode a turn taking in the queued `messages` starts with: the newest new-turn message's
/// own choice, the standard mode when it chose none.
pub(crate) fn queued(messages: &[AgentPendingMessage]) -> Option<TurnSpeedMode> {
    messages
        .iter()
        .rev()
        .find(|message| message.destination == AgentInputLane::NextTurn)?
        .speed_mode
        .clone()
}

/// The mode a step of the turn is sent with.
pub(super) fn turn_mode(
    chosen: Option<&TurnSpeedMode>,
    profile: &ModelProfileConfig,
) -> Option<ModelSpeedMode> {
    aura_model_domain::speed_mode_to_send(
        chosen.map(TurnSpeedMode::as_str),
        &profile.request_defaults,
        profile.catalog.as_ref(),
    )
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{EventId, Revision, TurnTaskId, UtcTimestamp};

    use super::*;

    fn message(destination: AgentInputLane, mode: Option<&str>) -> AgentPendingMessage {
        AgentPendingMessage {
            computer_mode: None,
            message_id: EventId::new(),
            revision: Revision::INITIAL,
            text: "queued".to_owned(),
            destination,
            created_at: UtcTimestamp::now(),
            skills: Vec::new(),
            reasoning_effort: None,
            speed_mode: mode.map(|name| TurnSpeedMode::parse(name).expect("mode")),
            attachments: Vec::new(),
            references: Vec::new(),
        }
    }

    #[test]
    fn a_turn_from_the_queue_takes_the_newest_new_turn_messages_own_choice() {
        let next = |mode| message(AgentInputLane::NextTurn, mode);
        let addition =
            |mode| message(AgentInputLane::Addition { turn_task_id: TurnTaskId::new() }, mode);
        let chosen = |messages: &[AgentPendingMessage]| {
            queued(messages).map(|mode| mode.as_str().to_owned())
        };
        assert_eq!(chosen(&[next(None), next(Some("fast"))]).as_deref(), Some("fast"));
        // The newest message chose the standard mode: an older one's choice does not carry over.
        assert_eq!(chosen(&[next(Some("fast")), next(None)]), None);
        // An addition to a running turn chooses nothing for the next one.
        assert_eq!(
            chosen(&[next(Some("ultrafast")), addition(Some("fast"))]).as_deref(),
            Some("ultrafast")
        );
        assert_eq!(chosen(&[addition(Some("fast"))]), None);
        assert_eq!(chosen(&[]), None);
    }

    #[test]
    fn a_mode_the_conversation_can_name_is_a_word_the_model_domain_takes() {
        for word in ["fast", "ultrafast", "fast-mode-2026-02-01", "v1.2_beta", &"a".repeat(64)] {
            assert!(TurnSpeedMode::parse(word).is_some(), "{word}");
            assert!(aura_model_domain::is_speed_mode_word(word), "{word}");
        }
        for word in ["", "fast mode", "fast\r\nx: 1", "速", &"a".repeat(65)] {
            assert!(TurnSpeedMode::parse(word).is_none(), "{word:?}");
            assert!(!aura_model_domain::is_speed_mode_word(word), "{word:?}");
        }
    }
}
