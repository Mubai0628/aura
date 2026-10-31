//! The one result evaluator for package HTTP responses: declared success statuses,
//! then the declared result contract. Conformance and real calls both use it.
use aura_tool_domain::CanonicalToolPackageToolV1;
use aura_tool_runtime::{PreparedToolResultContractV1, ValidatedToolResultBodyV1};
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageHttpEvaluation {
    /// The model-visible value: `null` for unit, a string for text, the JSON value.
    Completed { value: Value },
    /// The service answered with a status the package does not declare as success.
    UnexpectedStatus { status: u16 },
    /// The body does not satisfy the declared result contract.
    InvalidResult,
}

#[must_use]
pub fn evaluate(
    tool: &CanonicalToolPackageToolV1,
    success_ranges: &[(u16, u16)],
    status: u16,
    body: &[u8],
) -> PackageHttpEvaluation {
    evaluate_contract(&tool.model.result_contract, success_ranges, status, body)
}

/// [`evaluate`] for a frozen definition's result contract.
#[must_use]
pub fn evaluate_contract(
    contract: &aura_tool_contract::ToolResultContractV1,
    success_ranges: &[(u16, u16)],
    status: u16,
    body: &[u8],
) -> PackageHttpEvaluation {
    if !success_ranges.iter().any(|(start, end)| (*start..=*end).contains(&status)) {
        return PackageHttpEvaluation::UnexpectedStatus { status };
    }
    let Ok(contract) = PreparedToolResultContractV1::compile(contract) else {
        return PackageHttpEvaluation::InvalidResult;
    };
    match contract.validate_candidate(body) {
        Ok(ValidatedToolResultBodyV1::Unit) => {
            PackageHttpEvaluation::Completed { value: Value::Null }
        }
        Ok(ValidatedToolResultBodyV1::Utf8Text { text, .. }) => {
            PackageHttpEvaluation::Completed { value: Value::String(text) }
        }
        Ok(ValidatedToolResultBodyV1::StructuredJson { value, .. }) => {
            PackageHttpEvaluation::Completed { value }
        }
        Err(_) => PackageHttpEvaluation::InvalidResult,
    }
}
