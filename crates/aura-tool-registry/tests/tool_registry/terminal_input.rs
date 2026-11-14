//! What terminal.write sends is typed into the terminal as given: a command line runs once the
//! input ends with a newline, and without one it waits on the prompt. The write answers once the
//! shell's prompt is back, so a command that is silent for a moment is still one answer, and an
//! until_text that the typed line itself names is looked for after the line's echo.
use aura_tool_registry::agent_release::release;

#[test]
fn terminal_write_says_it_answers_once_the_shells_prompt_is_back() {
    let release = release().expect("verified current release");
    let description =
        &release.entry("terminal.write").expect("published").definition.model_description;
    assert!(
        description.contains(
            "Without until_text it returns the new output once the shell's prompt is back, or \
             after the output has been quiet for 3 seconds"
        ),
        "{description}"
    );
}

#[test]
fn terminal_write_says_until_text_is_looked_for_after_the_echo_of_the_typed_line() {
    let release = release().expect("verified current release");
    let description =
        &release.entry("terminal.write").expect("published").definition.model_description;
    assert!(
        description.contains(
            "with until_text, as soon as that text appears after the echo of the typed line"
        ),
        "{description}"
    );
}

#[test]
fn terminal_write_says_a_command_line_runs_once_its_input_ends_with_a_newline() {
    let release = release().expect("verified current release");
    let description =
        &release.entry("terminal.write").expect("published").definition.model_description;
    assert!(
        description.contains(
            "input is typed as given, so end it with a newline (\\n) to run a command line"
        ),
        "{description}"
    );
}
