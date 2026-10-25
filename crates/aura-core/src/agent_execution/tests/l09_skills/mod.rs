//! The user's Skills through the real Agent admission, policy, execution and result
//! path. Skill folders live in TEMP (the tests' stand-in for the user profile and in the test
//! workspace); trust goes through the real settings routes; only the Provider output is a
//! fixture.
use super::*;
mod changes;
mod support;
mod tools;
