# Logbook

A dependency-free log preview library. Run `cargo test --offline --locked`.
Headers are a nonnegative integer timestamp, an uppercase severity and a message.
Source order is meaningful; preview filters never sort records.
The repository-local Cargo settings also bound commands launched by an editor.
