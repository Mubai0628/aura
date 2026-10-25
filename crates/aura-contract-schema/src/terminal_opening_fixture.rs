//! Opening bridge samples are serialized from the owning versioned DTOs.
use anyhow::Result;
use aura_domain_common::ToolWireVersionV1;
use aura_windows_user_contract::{
    ManagedShell, TerminalOpeningChangeV1, TerminalOpeningPhaseV1, TerminalOpeningProgressV1,
    TerminalOpeningRefV1, TerminalOpeningStartV1, UserTerminalOpenRequestV1,
};
use serde_json::json;

pub(crate) const PATH: &str = "fixtures/contracts/terminal-opening.v1.json";

pub(crate) fn bytes() -> Result<Vec<u8>> {
    let opening_id = "00000000-0000-4000-8000-000000000001";
    let session_id = "ses_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?;
    let runtime_instance_id = "run_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?;
    let start = TerminalOpeningStartV1 {
        contract_version: ToolWireVersionV1,
        opening_id: opening_id.into(),
        request: UserTerminalOpenRequestV1 {
            session_id,
            shell: ManagedShell::PowerShell,
            columns: 120,
            rows: 30,
        },
    };
    let reference =
        TerminalOpeningRefV1 { contract_version: ToolWireVersionV1, opening_id: opening_id.into() };
    let mut progress = Vec::new();
    let mut changes = Vec::new();
    for phase in [
        TerminalOpeningPhaseV1::WaitingRuntime,
        TerminalOpeningPhaseV1::WaitingLaunch,
        TerminalOpeningPhaseV1::Launching,
        TerminalOpeningPhaseV1::Cancelled,
    ] {
        progress.push(TerminalOpeningProgressV1 {
            contract_version: ToolWireVersionV1,
            runtime_instance_id,
            request_id: "cor_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse()?,
            session_id,
            phase,
        });
        changes.push(TerminalOpeningChangeV1 {
            contract_version: ToolWireVersionV1,
            opening_id: opening_id.into(),
            runtime_instance_id,
            session_id,
            phase,
        });
    }
    crate::pretty_json(
        &json!({"start":start,"reference":reference,"progress":progress,"changes":changes}),
    )
}
