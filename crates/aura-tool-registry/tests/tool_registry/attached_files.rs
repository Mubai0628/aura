//! A file the user attached from outside the workspace is read at its full path: the user's grant
//! for it applies by itself, and no tool takes the grant's id.
use aura_tool_registry::agent_release::release;

#[test]
fn fs_read_says_a_user_authorized_external_file_is_read_at_its_full_path() {
    let release = release().expect("verified current release");
    let description = &release.entry("fs.read").expect("published").definition.model_description;
    assert!(
        description.contains("A user-authorized external file can be read at its full path."),
        "{description}"
    );
}
