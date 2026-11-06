//! The Chat Completions stream: chunks in, normalized events out.
mod delta;
mod state;
mod tools;
mod usage;

pub(crate) use state::ChatDecoder;
