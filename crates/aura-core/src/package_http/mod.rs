//! Package HTTP tools: the request builder and result evaluator shared by
//! Conformance and Agent calls. Transport and credentials stay with the caller.
mod conformance;
mod evaluate;
mod request;
mod synthesize;
pub use conformance::run_case;
pub use evaluate::{PackageHttpEvaluation, evaluate, evaluate_contract};
pub(crate) use request::declarative;
pub use request::{PackageHttpRequest, build};

#[cfg(test)]
mod tests;
