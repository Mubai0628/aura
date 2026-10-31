//! The thinking level the conversation
//! chose for a turn replaces the model's default in each of its steps' requests, when the model
//! offers it.
use aura_agent_domain::TurnReasoningEffort;
use aura_model_domain::{ModelApiFormat, ModelProfileConfig, ReasoningEffort};

/// The model domain's name for a level the agent domain carries.
pub(crate) const fn model_level(level: TurnReasoningEffort) -> ReasoningEffort {
    match level {
        TurnReasoningEffort::Off => ReasoningEffort::Off,
        TurnReasoningEffort::Minimal => ReasoningEffort::Minimal,
        TurnReasoningEffort::Low => ReasoningEffort::Low,
        TurnReasoningEffort::Medium => ReasoningEffort::Medium,
        TurnReasoningEffort::High => ReasoningEffort::High,
        TurnReasoningEffort::Xhigh => ReasoningEffort::Xhigh,
        TurnReasoningEffort::Max => ReasoningEffort::Max,
    }
}

/// The level a step sends: the turn's choice when the model offers it, else what the profile
/// resolved to (its default, or nothing).
pub(crate) fn turn_level(
    chosen: Option<TurnReasoningEffort>,
    profile: &ModelProfileConfig,
    api_format: ModelApiFormat,
    resolved: Option<ReasoningEffort>,
) -> Option<ReasoningEffort> {
    let offered = aura_model_domain::offered_reasoning_efforts(
        &profile.request_defaults,
        api_format,
        profile.catalog.as_ref(),
    );
    chosen
        .map(model_level)
        .and_then(|level| aura_model_domain::level_to_send(level, &offered))
        .or(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_level_the_conversation_can_choose_is_the_same_level_for_the_model() {
        let names: Vec<_> = TurnReasoningEffort::ALL
            .into_iter()
            .map(|level| {
                let turn = serde_json::to_value(level).expect("turn level");
                let model = serde_json::to_value(model_level(level)).expect("model level");
                assert_eq!(turn, model);
                turn
            })
            .collect();
        assert_eq!(
            names,
            ["off", "minimal", "low", "medium", "high", "xhigh", "max"]
                .map(serde_json::Value::from)
        );
    }
}
