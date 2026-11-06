//! A plugin's theme: `theme.toml` with the window's fixed colours, fonts and corner radii, each
//! value checked. A theme with a problem is not offered and the rest of the plugin still works;
//! a plugin may bring a theme and nothing else.
mod support;
use aura_plugins::{PluginThemeScheme, THEME_FILE, parse_theme, validation};
use support::{scan, write};

const MANIFEST: &str = "schema_version = 1\nname = \"night\"\nversion = \"1.0.0\"\n";

#[test]
fn a_theme_sets_only_the_fixed_colours_fonts_and_radii() {
    let theme = parse_theme(
        r##"
title = "夜间"
scheme = "dark"

[colors]
ink = "#E6E6E6"
canvas = "#1e1f1c"
line_strong = "#333"
accent_soft = "#3f625c80"

[fonts]
ui = ["Inter", "Microsoft YaHei UI", "思源黑体"]
mono = ["JetBrains Mono"]

[radius]
sm = 0
xl = 24
"##,
    )
    .expect("a theme");
    assert_eq!(theme.title, "夜间");
    assert_eq!(theme.scheme, PluginThemeScheme::Dark);
    assert_eq!(theme.colors.get("ink").map(String::as_str), Some("#e6e6e6"), "kept in lower case");
    assert_eq!(theme.colors.get("line_strong").map(String::as_str), Some("#333"));
    assert_eq!(theme.colors.len(), 4);
    assert_eq!(theme.fonts.ui, ["Inter", "Microsoft YaHei UI", "思源黑体"]);
    assert_eq!(theme.fonts.mono, ["JetBrains Mono"]);
    assert!(theme.fonts.serif.is_empty());
    assert_eq!(theme.radius.get("sm"), Some(&0));
    assert_eq!(theme.radius.get("xl"), Some(&24));

    // Without a scheme it is light; one kind of value is enough.
    let light = parse_theme("title = \"浅色圆角\"\n[radius]\nmd = 4\n").expect("a theme");
    assert_eq!(light.scheme, PluginThemeScheme::Light);
    assert!(light.colors.is_empty());
}

#[test]
fn every_unknown_key_and_bad_value_is_named_on_its_own() {
    let problems = parse_theme(
        r##"
title = "坏主题"
shadow = "big"

[colors]
ink = "red"
paper = "#fff"
accent = "#12345"

[fonts]
ui = ["Inter", "x;}body{display:none"]
code = ["Consolas"]
mono = []

[radius]
sm = 25
md = -1
huge = 4
lg = "8px"
"##,
    )
    .expect_err("problems");
    assert_eq!(problems.len(), 11, "{problems:#?}");
    for named in [
        "shadow",
        "ink",
        "paper",
        "accent",
        "x;}body{display:none",
        "code",
        "mono",
        "sm",
        "md",
        "huge",
        "lg",
    ] {
        assert!(problems.iter().any(|problem| problem.contains(named)), "{named}: {problems:#?}");
    }
}

#[test]
fn a_theme_needs_a_one_line_title_a_known_scheme_and_something_to_set() {
    for (text, named) in [
        ("[colors]\nink = \"#000\"\n", "title"),
        ("title = \"\"\n[colors]\nink = \"#000\"\n", "title"),
        ("title = \"两\\n行\"\n[colors]\nink = \"#000\"\n", "title"),
        (&format!("title = \"{}\"\n[colors]\nink = \"#000\"\n", "长".repeat(41)), "title"),
        ("title = \"x\"\nscheme = \"dim\"\n[colors]\nink = \"#000\"\n", "scheme"),
        ("title = \"空的\"\n", "没有颜色、字体或圆角"),
        ("title = \"x\"\ncolors = \"#000\"\n", "[colors]"),
        ("title = \"x\"\n[fonts]\nui = \"Inter\"\n", "ui"),
        (
            "title = 'x'\n[fonts]\nui = [\"a\", \"b\", \"c\", \"d\", \"e\", \"f\", \"g\", \"h\", \"i\"]\n",
            "ui",
        ),
        ("title = \"x\"\n[radius]\nsm = 1.5\n", "sm"),
        ("not toml at all [", "无法解析"),
    ] {
        let problems = parse_theme(text).expect_err(text);
        assert!(problems.iter().any(|problem| problem.contains(named)), "{text}: {problems:#?}");
    }
}

#[test]
fn a_plugin_may_bring_a_theme_and_nothing_else() {
    let temp = tempfile::tempdir().expect("temp");
    write(temp.path(), "aura-plugin.toml", MANIFEST);
    write(
        temp.path(),
        THEME_FILE,
        "title = \"夜间\"\nscheme = \"dark\"\n[colors]\ncanvas = \"#1e1f1c\"\n",
    );
    let package = scan(temp.path()).expect("a plugin");
    assert_eq!(package.counts().themes, 1);
    let item = package.theme_item().expect("it brings a theme");
    assert_eq!(item.title.as_deref(), Some("夜间"));
    assert!(item.problems.is_empty());
    assert_eq!(
        package.theme.as_ref().and_then(|theme| theme.as_ref().ok()).map(|t| t.scheme),
        Some(PluginThemeScheme::Dark)
    );
    assert!(validation(&package).problems.is_empty());
}

#[test]
fn a_broken_theme_is_not_offered_and_the_rest_of_the_plugin_works() {
    let temp = tempfile::tempdir().expect("temp");
    support::aura_plugin(temp.path());
    write(temp.path(), THEME_FILE, "title = \"坏\"\n[colors]\nink = \"red\"\n");
    let package = scan(temp.path()).expect("still a plugin");
    let item = package.theme_item().expect("it has a theme file");
    assert!(item.title.is_none(), "a theme with a problem is not offered");
    assert_eq!(item.problems.len(), 1, "{:?}", item.problems);
    let counts = package.counts();
    assert_eq!((counts.themes, counts.skills, counts.commands), (0, 1, 1));
    let checked = validation(&package);
    assert!(checked.installable);
    assert!(
        checked
            .problems
            .iter()
            .any(|problem| problem.starts_with("主题：") && problem.contains("ink")),
        "{:?}",
        checked.problems
    );

    // A theme that is not text is a problem of the theme alone too.
    std::fs::write(temp.path().join(THEME_FILE), [0xff, 0xfe, 0x00]).expect("written");
    let package = scan(temp.path()).expect("still a plugin");
    assert!(package.theme_item().expect("a theme file").title.is_none());
    assert_eq!(package.counts().skills, 1);
}

#[test]
fn a_claude_code_plugin_brings_no_theme() {
    let temp = tempfile::tempdir().expect("temp");
    support::claude_plugin(temp.path());
    write(temp.path(), THEME_FILE, "title = \"夜间\"\n[colors]\ncanvas = \"#1e1f1c\"\n");
    let package = scan(temp.path()).expect("a plugin");
    assert!(package.theme_item().is_none());
    assert_eq!(package.counts().themes, 0);
}

#[test]
fn changing_the_theme_of_a_plugin_in_development_needs_no_new_trust() {
    let temp = tempfile::tempdir().expect("temp");
    support::aura_plugin(temp.path());
    write(temp.path(), THEME_FILE, "title = \"夜间\"\n[colors]\ncanvas = \"#1e1f1c\"\n");
    let before = scan(temp.path()).expect("a plugin");
    write(temp.path(), THEME_FILE, "title = \"夜间\"\n[colors]\ncanvas = \"#101010\"\n");
    let after = scan(temp.path()).expect("a plugin");
    assert_ne!(after.fingerprint, before.fingerprint, "an installed copy is trusted again");
    assert_eq!(after.development_fingerprint, before.development_fingerprint);
}
