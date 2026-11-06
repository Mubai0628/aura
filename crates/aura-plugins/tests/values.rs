//! How `plugins.json` keeps the credentials of a plugin's secret settings: a list with each
//! setting's key as data, so a key such as `token` never becomes a field name there. The map
//! written before the list still reads.
use aura_domain_common::CredentialId;
use aura_plugins::PluginValues;
use serde_json::json;

#[test]
fn secret_settings_are_written_as_a_list_with_the_key_as_data() {
    let (token, password) = (CredentialId::new(), CredentialId::new());
    let mut values = PluginValues::default();
    values.secrets.insert("token".into(), token);
    values.secrets.insert("password".into(), password);
    let written = serde_json::to_value(&values).expect("written");
    assert_eq!(
        written,
        json!({"secrets": [
            {"key": "password", "secret_id": password},
            {"key": "token", "secret_id": token},
        ]})
    );
    let read: PluginValues = serde_json::from_value(written).expect("read back");
    assert_eq!(read, values);
}

#[test]
fn the_earlier_map_of_secret_settings_still_reads() {
    let id = CredentialId::new();
    let read: PluginValues =
        serde_json::from_value(json!({"text": {"app_id": "1024"}, "secrets": {"app_secret": id}}))
            .expect("the earlier form");
    assert_eq!(read.secrets.get("app_secret"), Some(&id));
    assert_eq!(read.text.get("app_id").map(String::as_str), Some("1024"));
}

#[test]
fn a_secret_setting_listed_twice_or_with_another_field_is_refused() {
    let (first, second) = (CredentialId::new(), CredentialId::new());
    for wrong in [
        json!({"secrets": [
            {"key": "token", "secret_id": first},
            {"key": "token", "secret_id": second},
        ]}),
        json!({"secrets": [{"key": "token", "secret_id": first, "value": "x"}]}),
        json!({"secrets": [{"key": "token"}]}),
    ] {
        assert!(serde_json::from_value::<PluginValues>(wrong.clone()).is_err(), "{wrong}");
    }
}
