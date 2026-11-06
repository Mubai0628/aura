//! A secret setting may have any key the manifest allows, `token` and `password` among them.
//! `plugins.json` keeps only its credential, with the key as data, so the check that keeps
//! secret-named fields out of every config file still passes and still guards the rest.
use super::support::{Fixture, write};
use aura_config::{AuraPaths, SettingsFacade};
use aura_windows_platform::SecretBytes;
use serde_json::Value;

const KEYS: [&str; 4] = ["token", "password", "api_key", "client_secret"];

fn manifest() -> String {
    let mut text = String::from(
        "schema_version = 2\nname = \"token-bot\"\nversion = \"0.1.0\"\n\
         description = \"保密设置的键像保密字段\"\ncapabilities = [\"status\"]\n\n\
         [service]\ncommand = \"${PLUGIN_ROOT}/service/run.cmd\"\n",
    );
    for key in KEYS {
        text.push_str(&format!(
            "\n[[settings]]\nkey = \"{key}\"\ntitle = \"{key}\"\nkind = \"secret\"\n"
        ));
    }
    text
}

fn value(key: &str) -> String {
    format!("fixture-{key}-value")
}

#[test]
fn a_secret_setting_named_like_a_secret_keeps_only_its_credential_in_plugins_json() {
    let f = Fixture::with_secrets();
    let source = f.source().join("token-bot");
    write(&source.join("aura-plugin.toml"), &manifest());
    write(&source.join("service/run.cmd"), "@echo off\r\n");
    f.import(&source).expect("imported");
    f.trust("token-bot", false).expect("trusted");
    for key in KEYS {
        let request =
            aura_plugins::PluginSecretSetRequest { name: "token-bot".into(), key: key.into() };
        f.runtime
            .set_secret(&request, &SecretBytes::new(value(key).into_bytes()))
            .unwrap_or_else(|error| panic!("{key}: {error:#}"));
    }
    assert_eq!(f.secrets().len(), KEYS.len());
    let file = std::fs::read_to_string(f.root.path().join("config").join("plugins.json"))
        .expect("plugins.json");
    for key in KEYS {
        assert!(!file.contains(&value(key)), "{key}'s value never reaches plugins.json");
    }
    let document: Value = serde_json::from_str(&file).expect("plugins.json is JSON");
    let plugin = document["plugins"]
        .as_array()
        .expect("plugins")
        .iter()
        .find(|plugin| plugin["name"] == "token-bot")
        .expect("token-bot");
    let mut listed: Vec<&str> = plugin["values"]["secrets"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry["key"].as_str().expect("the key as data"))
        .collect();
    listed.sort_unstable();
    let mut expected = KEYS.to_vec();
    expected.sort_unstable();
    assert_eq!(listed, expected);
    let reopened = SettingsFacade::open(AuraPaths::from_roots(
        f.root.path().join("config"),
        f.root.path().join("local"),
    ))
    .expect("settings open again");
    let saved = reopened.plugin_registry().plugin("token-bot").cloned().expect("saved");
    assert_eq!(saved.values.secrets.len(), KEYS.len());
    assert_eq!(saved.values.secrets, f.saved("token-bot").expect("saved").values.secrets);
}
