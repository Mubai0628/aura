use aura_prompt_runtime::ValidatedPromptBundle;

#[test]
fn prompts_prefer_the_file_tool_for_cleanup_in_restricted_modes() {
    let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
    let module = |id| {
        &bundle
            .modules
            .iter()
            .find(|module| module.module_id == id)
            .expect("prompt module")
            .normalized_content
    };
    let files = module("capabilities.files");
    assert!(files.contains("including temporary files you created"));
    assert!(
        files.contains("permanent: true")
            && files.contains("only if the user wants irreversible deletion")
    );
    let shell = module("capabilities.shell-terminal-process");
    assert!(shell.contains("In 只读 and 工作区读写, prefer fs.delete for routine cleanup."));
    assert!(shell.contains("separate file-tool call"));
    assert!(shell.contains("Outside 完全访问, Aura asks before each Shell deletion."));
    assert!(shell.contains("has authorized irreversible deletion"));
}
