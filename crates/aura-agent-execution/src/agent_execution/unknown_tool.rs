//! Feedback is derived only from the exact request evidence, never a live registry.
use aura_domain_common::ToolCallId;
use aura_model_domain::{NormalizedModelOutput, ProviderRequestEvidence};
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, NormalizedToolCallV1, TOOLS_CALL_NAME, ToolRegistrySnapshotV1,
    entry_callable, is_entry_call,
};
use std::collections::BTreeMap;

pub(super) fn unknown_tool_feedback(
    evidence: &ProviderRequestEvidence,
    output: &NormalizedModelOutput,
) -> BTreeMap<ToolCallId, String> {
    let listed = evidence
        .tool_name_bindings
        .iter()
        .map(|b| b.provider_wire_name.as_str())
        .collect::<Vec<_>>();
    let available = listed.join(", ");
    // A tool that is not in the list is reached through the unified entry.
    let through_entry =
        evidence.tool_name_bindings.iter().any(|b| b.canonical_name == TOOLS_CALL_NAME);
    let entry = if through_entry {
        " Other tools of this step are called through tools.call with the canonical_name tools.search gives."
    } else {
        ""
    };
    output.tool_calls.iter().filter(|call| {
        !evidence.tool_name_bindings.iter().any(|b| b.provider_wire_name == call.tool_name)
    }).map(|call| {
        let name = call.tool_name.chars().take(64).map(|c| if c.is_control() { '_' } else { c })
            .collect::<String>();
        let meant = match same_first_part(&listed, &call.tool_name) {
            Some(one) => format!(" The listed tool `{one}` has the same name before \"__\"; call it by that exact name."),
            None if through_entry => entry_hint(&name),
            None => String::new(),
        };
        (call.tool_call_id, format!("No such tool in this step: `{name}`. This call was not executed.{meant} Available tools in this step: {available}. Use an exact listed name.{entry}"))
    }).collect()
}

/// The one listed tool whose wire name matches a mistyped one up to its `__`: a model that copies
/// the long suffix wrong still names the tool by the part before it.
fn same_first_part<'a>(listed: &[&'a str], wire: &str) -> Option<&'a str> {
    let (base, _) = wire.split_once("__")?;
    let mut found =
        listed.iter().copied().filter(|one| one.split_once("__").is_some_and(|(b, _)| b == base));
    let one = found.next()?;
    found.next().is_none().then_some(one)
}

/// A name of Aura's wire form names the tool, which tools.call runs.
fn entry_hint(wire: &str) -> String {
    let Some((base, suffix)) = wire.split_once("__") else { return String::new() };
    if suffix.is_empty() || !base.contains('_') {
        return String::new();
    }
    let canonical = base.replacen('_', ".", 1);
    if !entry_callable(&canonical) {
        return String::new();
    }
    format!(
        " If you meant {canonical}, it is not loaded in this step: call tools.call with {{\"tool\": \"{canonical}\", \"arguments\": {{...its arguments...}}}}."
    )
}

/// Why a `tools.call` that stayed bound to the entry ran nothing.
pub(super) fn entry_feedback(
    registry: &ToolRegistrySnapshotV1,
    calls: &[NormalizedToolCallV1],
) -> BTreeMap<ToolCallId, String> {
    calls
        .iter()
        .filter(|call| is_entry_call(registry, call))
        .map(|call| {
            let named = match &call.input {
                NormalizedToolCallInputStateV1::Ready { arguments, .. } => {
                    arguments["tool"].as_str().unwrap_or_default()
                }
                NormalizedToolCallInputStateV1::InvalidInput { .. } => "",
            };
            let named = named
                .chars()
                .take(64)
                .map(|c| if c.is_control() { '_' } else { c })
                .collect::<String>();
            (
                call.tool_call_id,
                format!(
                    "tools.call did not run `{named}`: no tool of this step by that name can be called through tools.call. Use the canonical_name from tools.search (for example git.commit) with that tool's own arguments, and call tools in this request's list directly. Registry and turn-control tools (tools.*, skills.*, plan.*, goal.*, mode.*, normal.*, query.*, task_lineage.*, workspace_init.*) cannot be called through tools.call."
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_wire_name_of_a_tool_not_sent_points_to_the_entry() {
        let hint = super::entry_hint("browser_select__01m2dg020980080021an94202b");
        assert!(hint.contains("browser.select"), "{hint}");
        assert!(hint.contains(r#"{"tool": "browser.select""#), "{hint}");
        assert!(super::entry_hint("made_up").is_empty());
        assert!(super::entry_hint("tools_load__01arz3ndektsv4rrffq69g5fax").is_empty());
    }

    #[test]
    fn a_name_with_a_mistyped_suffix_points_to_the_one_listed_tool_it_means() {
        let listed = [
            "tools_search__01arz3ndektsv4rrffq69g5fay",
            "fs_read__01m2dg020980080021an942001",
            "search__mcp_one",
            "search__mcp_two",
        ];
        // The suffix as a model once mixed two tools' ids.
        assert_eq!(
            super::same_first_part(&listed, "tools_search__01m2dg020980080021an94200g5fay"),
            Some("tools_search__01arz3ndektsv4rrffq69g5fay")
        );
        // Two tools share the first part: neither is guessed.
        assert_eq!(super::same_first_part(&listed, "search__mcp_three"), None);
        assert_eq!(super::same_first_part(&listed, "fs_write__01m2dg020980080021an942003"), None);
        assert_eq!(super::same_first_part(&listed, "tools_search"), None);
    }
}
