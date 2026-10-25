//! A rebuilt request must make measurable progress before returning to Provider.
use aura_context_domain::*;
use aura_persistence_v2::model::context_window::WindowCorrection;

pub(super) fn no_progress(
    trial: Option<&WindowCorrection>,
    snapshot: &ContextProjectionSnapshot,
) -> Option<ContextRemediationPlan> {
    let trial = trial?;
    let aura_model_domain::ContextWindowResolution::P06RebuildRequired {
        local_estimated_input_tokens,
        ..
    } = trial.view.correction.resolution
    else {
        return None;
    };
    if snapshot.budget.estimated_input_tokens < local_estimated_input_tokens {
        return None;
    }
    let mut non_compressible = 0u64;
    let mut reducible = 0u64;
    for section in &snapshot.sections {
        if matches!(
            section.compressibility,
            ContextCompressibility::NeverCompress | ContextCompressibility::UserReducibleOnly
        ) {
            non_compressible = non_compressible.saturating_add(section.estimated_tokens);
        }
        if section.compressibility == ContextCompressibility::UserReducibleOnly {
            reducible = reducible.saturating_add(section.estimated_tokens);
        }
    }
    let actions = vec!["reduce_user_managed_context".into()];
    Some(ContextRemediationPlan {
        overflow: ContextOverflowDiagnosticView {
            request_input_budget_tokens: snapshot.budget.request_input_budget_tokens,
            estimated_input_tokens: snapshot.budget.estimated_input_tokens,
            non_compressible_tokens: non_compressible,
            compressible_history_tokens: snapshot
                .budget
                .estimated_input_tokens
                .saturating_sub(non_compressible),
            user_reducible_tokens: reducible,
            overflow_tokens: snapshot
                .budget
                .estimated_input_tokens
                .saturating_sub(snapshot.budget.request_input_budget_tokens),
            error_code: "context.provider_rebuild_no_progress".into(),
            remediation: actions.clone(),
        },
        actions,
    })
}
