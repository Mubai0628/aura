//! The prompt modules leave the way of working to the user: none tells the model to tidy up after
//! itself, and a habit such as leaving local paths out of a command gives way when the user wants
//! otherwise. Secrets stay out whatever the user says.
use aura_prompt_runtime::ValidatedPromptBundle;

#[test]
fn no_prompt_module_makes_the_model_tidy_up_after_itself() {
    let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
    for module in &bundle.modules {
        let text = module.normalized_content.to_lowercase();
        for tidy in
            ["once you are done", "when you are done", "when done with", "clean up", "tidy up"]
        {
            assert!(!text.contains(tidy), "{}: {tidy}: {text}", module.module_id);
        }
    }
}

#[test]
fn a_command_keeps_secrets_out_and_takes_local_paths_when_the_user_wants_them() {
    let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
    let authoring = bundle
        .modules
        .iter()
        .find(|module| module.module_id == "capabilities.command-authoring")
        .expect("the command authoring module");
    let text = &authoring.normalized_content;
    assert!(text.contains("never include secrets"), "{text}");
    assert!(
        text.contains(
            "leave out local paths and anything that only holds today unless the user wants them"
        ),
        "{text}"
    );
    assert!(!text.contains("never include secrets, local paths"), "{text}");
}
