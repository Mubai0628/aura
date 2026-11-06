//! A plugin's log: bounded, newest last, read with the secret values it was given hidden, and
//! removed with the plugin.
use super::super::logs::hidden;
use super::support::{Fixture, plugin};
use aura_agent_domain::PluginLogSource;

#[test]
fn entries_come_back_oldest_first_and_only_the_newest_are_kept_in_a_reading() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    for n in 1..=3 {
        f.runtime.log("review-kit", PluginLogSource::Panel, "本会话用量", &format!("error {n}"));
    }
    let (entries, truncated) = f.runtime.recent_logs("review-kit", 2).expect("logs");
    assert!(truncated);
    let texts: Vec<&str> = entries.iter().map(|entry| entry.text.as_str()).collect();
    assert_eq!(texts, ["error 2", "error 3"]);
    assert_eq!(entries[0].source, PluginLogSource::Panel);
    assert_eq!(entries[0].part, "本会话用量");
}

#[test]
fn a_full_file_moves_aside_and_both_are_read() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    let long = "x".repeat(3900);
    for n in 0..80 {
        f.runtime.log("review-kit", PluginLogSource::Service, "service", &format!("{n} {long}"));
    }
    let folder = f.runtime.root().join("logs");
    assert!(folder.join("review-kit.1.jsonl").is_file(), "the older file");
    let current = std::fs::metadata(folder.join("review-kit.jsonl")).expect("current").len();
    assert!(current <= 256 * 1024, "{current}");
    let (entries, _) = f.runtime.recent_logs("review-kit", 200).expect("logs");
    assert!(entries.len() > 60, "{}", entries.len());
    assert!(entries.last().expect("newest").text.starts_with("79 "));
}

#[test]
fn secret_values_including_short_ones_are_hidden() {
    let secrets = vec!["sk-abcdef123456".to_owned(), "ab".to_owned()];
    assert_eq!(hidden("token sk-abcdef123456 refused; ab", &secrets), "token •••• refused; ••••");
}

#[test]
fn overlapping_values_and_mask_characters_do_not_rescan_generated_masks() {
    assert_eq!(hidden("abcdef •", &["abc".into(), "abcdef".into(), "•".into()]), "•••• ••••");
    assert_eq!(hidden("text", &[String::new()]), "text");
}

#[test]
fn uninstalling_removes_the_log_and_an_unknown_plugin_has_none() {
    let f = Fixture::new();
    f.import(&plugin(&f.source(), "review-kit", "1.0.0")).expect("imported");
    f.runtime.log("review-kit", PluginLogSource::Hook, "check", "ExitCode");
    f.change("agent.plugins.uninstall", "review-kit").expect("uninstalled");
    assert!(!f.runtime.root().join("logs").join("review-kit.jsonl").exists());
    let refused = f.runtime.recent_logs("review-kit", 10).expect_err("gone").to_string();
    assert!(refused.starts_with("plugins.not_found"), "{refused}");
}
