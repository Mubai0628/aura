//! Tool descriptions leave the way of working to the user: none tells the model to tidy up after
//! itself, since a user may want what was changed left as it is, and a default way of working gives
//! way to the one the user asked for. A rule holds whatever the user says only where Aura holds it
//! too: passwords, payments and secrets.
use aura_tool_registry::agent_release::release;

const COMPUTER_TOOLS: [&str; 7] = [
    "computer.apps",
    "computer.open",
    "computer.snapshot",
    "computer.screenshot",
    "computer.act",
    "computer.input",
    "computer.window",
];

fn description(name: &str) -> String {
    let release = release().expect("verified current release");
    release.entry(name).expect("published").definition.model_description.clone()
}

#[test]
fn no_tool_description_makes_the_model_tidy_up_after_itself() {
    let release = release().expect("verified current release");
    for entry in &release.manifest().entries {
        let description = entry.definition.model_description.to_lowercase();
        for tidy in
            ["once you are done", "when you are done", "when done with", "clean up", "tidy up"]
        {
            assert!(!description.contains(tidy), "{}: {tidy}: {description}", entry.handler.name());
        }
    }
}

#[test]
fn the_computer_tools_ways_of_working_give_way_to_the_users() {
    for name in COMPUTER_TOOLS {
        let description = description(name);
        assert!(
            description.contains(
                "unless the user said otherwise, do in one call the steps you already know"
            ),
            "{name}: {description}"
        );
    }
    let input = description("computer.input");
    assert!(!input.contains("use it only when"), "{input}");
    assert!(
        input.contains("unless the user asked for them, use computer.act for the steps it can do"),
        "{input}"
    );
}

#[test]
fn the_safety_rules_of_the_computer_tools_stay_whatever_the_user_says() {
    for name in ["computer.act", "computer.input", "computer.window"] {
        let description = description(name);
        assert!(
            description.contains("Never type or set a value into a password field.")
                && description.contains("Never pay, transfer money, trade or buy.")
                && description.contains("never repeat the step on your own"),
            "{name}: {description}"
        );
    }
    assert!(description("computer.window").contains("never choose not to save on your own"));
}

#[test]
fn memory_never_keeps_secrets_and_keeps_other_things_the_user_asks_it_to() {
    let create = description("memory.create");
    assert!(create.contains("Never store secrets."), "{create}");
    assert!(
        create.contains(
            "Leave out transient details and unverified claims unless the user asks to keep them."
        ),
        "{create}"
    );
    assert!(!create.contains("Never store secrets, transient details"), "{create}");
}
