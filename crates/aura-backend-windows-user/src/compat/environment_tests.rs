use super::environment::{
    VariableClass, build_declared, check_declared, classify, excluded_names, secret_values,
};
use std::ffi::{OsStr, OsString};

fn class(name: &str, value: &str) -> VariableClass {
    classify(name, OsStr::new(value))
}

fn source() -> Vec<(OsString, OsString)> {
    [
        ("Path", "C:\\tools"),
        ("MYSQL_PASSWORD", "mysql-fixture-pass"),
        ("SANDBOX_TOKEN", "sandbox-fixture-token"),
        ("AURA_CONTROL_TOKEN", "internal-fixture"),
        ("DATABASE_URL", "mysql://aioj:url-fixture-pw@127.0.0.1:3306/aioj"),
        ("HTTPS_PROXY", "http://me:proxy-fixture-pw@127.0.0.1:8080"),
        ("TOKENIZERS_PARALLELISM", "false"),
    ]
    .map(|(name, value)| (name.into(), value.into()))
    .into()
}

#[test]
fn credentials_are_recognised_by_name_and_by_a_password_in_an_address() {
    for name in [
        "DB_PASS",
        "SMTP_PASSWD",
        "GPG_PASSPHRASE",
        "DB_PWD",
        "API_SECRETS",
        "NPM_TOKENS",
        "SERVICE_PASSWORDS",
        "PGPASSWORD",
        "JWT_HMAC_SECRET",
        "AIOJ_INTERNAL_API_TOKEN",
    ] {
        assert_eq!(class(name, "x"), VariableClass::Secret, "{name}");
    }
    for name in ["PWD", "OLDPWD", "TOKENIZERS_PARALLELISM", "PASSAGE_DIR", "KEYBOARD_LAYOUT"] {
        assert_eq!(class(name, "x"), VariableClass::Plain, "{name}");
    }
    for name in ["AURA_ANYTHING", "codex_home", "__COMPAT_LAYER"] {
        assert_eq!(class(name, "x"), VariableClass::Internal, "{name}");
    }
    assert_eq!(class("DATABASE_URL", "postgres://admin:pw@db/app"), VariableClass::Secret);
    assert_eq!(class("DATABASE_URL", "postgres://admin@db/app"), VariableClass::Plain);
    assert_eq!(class("BACKUP_DIR", "C:\\Users\\me@corp"), VariableClass::Plain);
    assert_eq!(class("HTTPS_PROXY", "http://me:pw@proxy:8080"), VariableClass::Plain, "passed");
}

#[test]
fn a_declared_credential_is_passed_and_an_undeclared_one_or_aura_own_is_not() {
    let declared = ["mysql_password".to_owned(), "AURA_CONTROL_TOKEN".to_owned()];
    let entries = build_declared(source(), None, &declared).expect("environment");
    let names = entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>();
    assert!(names.contains(&"MYSQL_PASSWORD"), "declared, in any case: {names:?}");
    assert!(names.contains(&"HTTPS_PROXY") && names.contains(&"TOKENIZERS_PARALLELISM"));
    for absent in ["SANDBOX_TOKEN", "DATABASE_URL", "AURA_CONTROL_TOKEN"] {
        assert!(!names.contains(&absent), "{absent}: {names:?}");
    }
    assert_eq!(
        excluded_names(&source()),
        ["MYSQL_PASSWORD", "SANDBOX_TOKEN", "AURA_CONTROL_TOKEN", "DATABASE_URL"]
    );
}

#[test]
fn a_declaration_of_a_missing_or_internal_variable_is_refused_before_running() {
    let trusted = source();
    assert!(check_declared(&trusted, &["SANDBOX_TOKEN".into(), "Path".into()]).is_ok());
    let missing =
        check_declared(&trusted, &["AIOJ_INTERNAL_API_TOKEN".into()]).expect_err("missing");
    assert!(
        missing.contains("AIOJ_INTERNAL_API_TOKEN") && missing.contains("not set"),
        "{missing}"
    );
    let internal = check_declared(&trusted, &["AURA_CONTROL_TOKEN".into()]).expect_err("internal");
    assert!(internal.contains("never passed"), "{internal}");
}

#[test]
fn every_credential_value_and_a_proxy_password_are_hidden() {
    let values = secret_values(&source());
    let hidden = values.iter().map(|(_, value)| value.as_str()).collect::<Vec<_>>();
    for value in [
        "mysql-fixture-pass",
        "sandbox-fixture-token",
        "mysql://aioj:url-fixture-pw@127.0.0.1:3306/aioj",
        "url-fixture-pw",
        "proxy-fixture-pw",
    ] {
        assert!(hidden.contains(&value), "{value}: {hidden:?}");
    }
    assert!(!hidden.contains(&"internal-fixture") && !hidden.contains(&"false"));
}
