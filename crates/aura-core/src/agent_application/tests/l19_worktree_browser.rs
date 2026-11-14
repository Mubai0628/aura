//! The main agent shows the user a file its sub-agent changed in its worktree before anything is
//! merged: the browser opens the file from the sub-agent's copy, and the page may load only the
//! copy's files.
use super::l13_browser::{Host, play_host, set_browser};
use super::l19_team::{requests, results, turn};
use super::l19_worktrees::{only_copy, prepare};
use super::*;
use aura_browser_domain::BrowserHostActionV1;
use provider_fixture::{ADDED_FILE, Scenario, TeamPlan};

#[tokio::test]
async fn the_main_agent_opens_a_file_of_its_sub_agents_worktree_in_the_browser() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::OpenCopy }).await;
    let (runtime, session, model, root, location) = prepare(&host, &provider).await;
    set_browser(&runtime, |_| ());
    let (seen, _) = play_host(&runtime, Host::Answers);
    turn(&runtime, session, model, "改完先让我看看效果").await;

    let copy = only_copy(&location);
    let key = copy.file_name().and_then(|name| name.to_str()).expect("its key").to_lowercase();
    let commands = seen.lock().expect("seen").clone();
    let (url, file_root) = commands
        .iter()
        .find_map(|command| match &command.action {
            BrowserHostActionV1::Open { url } => {
                Some((url.to_lowercase(), command.access.file_root.clone().unwrap_or_default()))
            }
            _ => None,
        })
        .expect("the browser opened the file");
    assert!(url.starts_with("file:///") && url.ends_with(&format!("/{key}/{ADDED_FILE}")), "{url}");
    let file_root = file_root.to_lowercase();
    assert!(file_root.ends_with(&format!("/{key}/")), "only the copy's files: {file_root}");
    assert!(!root.join(ADDED_FILE).exists(), "nothing was merged");
    // The model was given the page.
    let (_, ours) = requests(&provider);
    let read = results(ours.last().expect("the conversation's last request"));
    let opened = read.last().expect("the browser call's result");
    assert!(!opened.contains("browser.open:"), "{opened}");
}
