//! The shared error table's own rows. Replies are matched in lower case, so a phrase written
//! otherwise would never match; and each account setting has a code of its own, which the
//! Renderer explains to the user.
use aura_provider_profile::{AccountSetting, account_setting, all_phrases};

#[test]
fn every_phrase_is_lower_case_trimmed_and_not_empty() {
    let all = all_phrases();
    assert!(all.len() >= 9);
    for (kind, phrases) in all {
        assert!(!phrases.is_empty(), "{kind}");
        for fragments in phrases {
            assert!(!fragments.is_empty(), "{kind}");
            for fragment in *fragments {
                assert!(!fragment.is_empty(), "{kind}");
                assert_eq!(*fragment, fragment.to_lowercase(), "{kind}");
                assert_eq!(*fragment, fragment.trim(), "{kind}");
            }
        }
    }
}

#[test]
fn each_account_setting_has_its_own_code() {
    let settings =
        [AccountSetting::Region, AccountSetting::DataPolicy, AccountSetting::ClientNotAllowed];
    let mut codes: Vec<&str> = settings.iter().map(|setting| setting.code()).collect();
    for code in &codes {
        assert!(code.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_'), "{code}");
    }
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), settings.len());
}

#[test]
fn an_account_setting_is_read_from_the_words_in_any_case_and_from_nothing_else() {
    let region = "This Go model requires Global regions. Select Global in your workspace's Privacy settings to use it.";
    assert_eq!(account_setting(&[], region), Some(AccountSetting::Region));
    assert_eq!(account_setting(&[], &region.to_uppercase()), Some(AccountSetting::Region));
    // Either half of OpenCode's sentence names the setting: its documentation words it so too.
    for words in [
        "This Go model requires Global regions: in Settings › Privacy, select Global under Regions.",
        "Your workspace's Privacy settings do not allow the regions this model is served from.",
    ] {
        assert_eq!(account_setting(&[], words), Some(AccountSetting::Region), "{words}");
    }
    for words in [
        "Kimi For Coding is currently only available for Coding Agents such as Kimi CLI",
        "This credential is only authorized for use with Claude Code and cannot be used for other API requests.",
    ] {
        assert_eq!(
            account_setting(&["access_terminated_error"], words),
            Some(AccountSetting::ClientNotAllowed),
            "{words}"
        );
    }
    // The training switch is named next to the regions; it is the data policy that refused.
    assert_eq!(
        account_setting(
            &[],
            "Turn on Allow paid endpoints that train on request data, or select Global regions, in your workspace's Privacy settings."
        ),
        Some(AccountSetting::DataPolicy)
    );
    for words in [
        "Rate limit reached for requests",
        "The model is not available in your region.",
        "Check the privacy settings of your browser.",
        "Unsupported country, region, or territory",
    ] {
        assert_eq!(account_setting(&["server_error"], words), None, "{words}");
    }
}
