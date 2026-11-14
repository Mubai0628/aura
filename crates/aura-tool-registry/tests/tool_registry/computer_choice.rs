//! When the computer tools are the way to do a task: the way the user asked for comes first, and
//! when they did not say how, the way that costs least time and tokens.
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

#[test]
fn the_computer_tools_take_the_users_way_first_and_otherwise_the_cheapest() {
    let release = release().expect("verified current release");
    for name in COMPUTER_TOOLS {
        let description =
            &release.entry(name).expect("computer tool is published").definition.model_description;
        assert!(
            description
                .contains("asked you to operate a program, operate it with the computer tools"),
            "{name}: a program the user asked to be operated is operated: {description}"
        );
        assert!(
            description.contains("costs least time and tokens"),
            "{name}: without the user's way, the cheapest way: {description}"
        );
        assert!(
            !description.contains("Do not use them for what"),
            "{name}: no rule puts another way above the user's: {description}"
        );
    }
}

#[test]
fn every_computer_call_waits_for_a_model_step_so_one_call_carries_what_it_can() {
    let release = release().expect("verified current release");
    for name in COMPUTER_TOOLS {
        let description =
            &release.entry(name).expect("computer tool is published").definition.model_description;
        assert!(
            description.contains("Every call waits for a model step")
                && description.contains("do in one call the steps you already know")
                && description.contains("calls that do not depend on each other together"),
            "{name}: {description}"
        );
    }
}

#[test]
fn opening_a_program_brings_its_window_to_the_front() {
    let release = release().expect("verified current release");
    let description =
        &release.entry("computer.open").expect("published").definition.model_description;
    assert!(
        description.starts_with("Open the program and bring its window to the front"),
        "{description}"
    );
    assert!(description.contains("is not opened again"), "{description}");
}
