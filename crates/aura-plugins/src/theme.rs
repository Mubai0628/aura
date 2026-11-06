//! A plugin's theme (`theme.toml`): the window's fixed colours, fonts and corner radii and
//! nothing else, each value checked here. The window builds its style sheet from these checked
//! values only, so a theme can change how Aura looks but never what it shows or runs.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The colours a theme may set, by the names of the window's own.
pub const THEME_COLORS: [&str; 19] = [
    "ink",
    "muted",
    "quiet",
    "line",
    "line_strong",
    "canvas",
    "surface",
    "surface_muted",
    "surface_selected",
    "accent",
    "accent_strong",
    "accent_soft",
    "success",
    "warning",
    "danger",
    "focus",
    "success_soft",
    "warning_soft",
    "danger_soft",
];
/// The corner radii a theme may set, in whole pixels.
pub const THEME_RADII: [&str; 4] = ["sm", "md", "lg", "xl"];
pub const THEME_MAX_RADIUS: u8 = 24;
/// The most fonts in one list, first choice first.
pub const THEME_MAX_FONTS: usize = 8;
pub const THEME_TITLE_MAX_CHARS: usize = 40;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginThemeScheme {
    /// Light controls and scroll bars.
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginThemeFonts {
    /// The interface and the conversation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ui: Vec<String>,
    /// Code, paths and commands.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mono: Vec<String>,
    /// Large titles and thinking.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub serif: Vec<String>,
}

/// A checked theme. Colours are `#rgb`, `#rrggbb` or `#rrggbbaa` in lower case, keyed by
/// `THEME_COLORS`; radii are keyed by `THEME_RADII`. What it does not set stays Aura's own.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PluginTheme {
    /// Its name in Settings.
    pub title: String,
    pub scheme: PluginThemeScheme,
    pub colors: BTreeMap<String, String>,
    pub fonts: PluginThemeFonts,
    pub radius: BTreeMap<String, u8>,
}

const FONT_NAME_MAX_CHARS: usize = 64;
/// How much of an unknown key or a bad value a problem repeats.
const SHOWN_MAX_CHARS: usize = 40;

/// Reads `theme.toml`. Every unknown key and bad value is one problem; a theme with any problem
/// is not used.
pub fn parse_theme(text: &str) -> Result<PluginTheme, Vec<String>> {
    let table: toml::Table = text.parse().map_err(|error: toml::de::Error| {
        let error = error.to_string();
        vec![format!("{} 无法解析：{}", crate::THEME_FILE, error.lines().next().unwrap_or(""))]
    })?;
    let mut problems = Vec::new();
    let mut theme = PluginTheme {
        title: String::new(),
        scheme: PluginThemeScheme::Light,
        colors: BTreeMap::new(),
        fonts: PluginThemeFonts::default(),
        radius: BTreeMap::new(),
    };
    for (key, value) in &table {
        match key.as_str() {
            "title" => match value.as_str().map(str::trim) {
                Some(title) if one_line(title, THEME_TITLE_MAX_CHARS) => {
                    title.clone_into(&mut theme.title)
                }
                _ => problems
                    .push(format!("title 要是一行不空的文字，最多 {THEME_TITLE_MAX_CHARS} 个字符")),
            },
            "scheme" => match value.as_str() {
                Some("light") => theme.scheme = PluginThemeScheme::Light,
                Some("dark") => theme.scheme = PluginThemeScheme::Dark,
                _ => problems.push("scheme 只能是 light 或 dark".into()),
            },
            "colors" => colors(value, &mut theme.colors, &mut problems),
            "fonts" => fonts(value, &mut theme.fonts, &mut problems),
            "radius" => radii(value, &mut theme.radius, &mut problems),
            other => problems.push(format!("不认识的键 {}", shown(other))),
        }
    }
    if !table.contains_key("title") {
        problems.push("缺少 title：主题在设置里显示的名字".into());
    }
    let fonts = &theme.fonts;
    let nothing = theme.colors.is_empty()
        && theme.radius.is_empty()
        && fonts.ui.is_empty()
        && fonts.mono.is_empty()
        && fonts.serif.is_empty();
    if problems.is_empty() && nothing {
        problems.push("主题里没有颜色、字体或圆角".into());
    }
    if problems.is_empty() { Ok(theme) } else { Err(problems) }
}

fn colors(value: &toml::Value, colors: &mut BTreeMap<String, String>, problems: &mut Vec<String>) {
    let Some(table) = value.as_table() else {
        problems.push("[colors] 要是一张表".into());
        return;
    };
    for (key, value) in table {
        if !THEME_COLORS.contains(&key.as_str()) {
            problems.push(format!("[colors] 里不认识的颜色 {}", shown(key)));
            continue;
        }
        match value.as_str().filter(|text| hex_color(text)) {
            Some(text) => {
                colors.insert(key.clone(), text.to_ascii_lowercase());
            }
            None => problems.push(format!("[colors] {key} 要是 #rgb、#rrggbb 或 #rrggbbaa")),
        }
    }
}

fn hex_color(text: &str) -> bool {
    text.strip_prefix('#').is_some_and(|digits| {
        matches!(digits.len(), 3 | 6 | 8) && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn fonts(value: &toml::Value, fonts: &mut PluginThemeFonts, problems: &mut Vec<String>) {
    let Some(table) = value.as_table() else {
        problems.push("[fonts] 要是一张表".into());
        return;
    };
    for (key, value) in table {
        let slot = match key.as_str() {
            "ui" => &mut fonts.ui,
            "mono" => &mut fonts.mono,
            "serif" => &mut fonts.serif,
            other => {
                problems.push(format!("[fonts] 里不认识的字体 {}", shown(other)));
                continue;
            }
        };
        let Some(names) =
            value.as_array().filter(|names| (1..=THEME_MAX_FONTS).contains(&names.len()))
        else {
            problems.push(format!("[fonts] {key} 要是 1 到 {THEME_MAX_FONTS} 个字体名的列表"));
            continue;
        };
        let mut checked = Vec::with_capacity(names.len());
        for name in names {
            match name.as_str().map(str::trim).filter(|name| font_name(name)) {
                Some(name) => checked.push(name.to_owned()),
                None => problems.push(format!(
                    "[fonts] {key} 里的 {} 不是字体名：只能有文字、数字、空格和 - _ .",
                    shown(&name.as_str().map_or_else(|| name.to_string(), str::to_owned))
                )),
            }
        }
        if checked.len() == names.len() {
            *slot = checked;
        }
    }
}

/// A font's name, which the window puts in quotes: nothing that could end the quotes or the
/// declaration.
fn font_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= FONT_NAME_MAX_CHARS
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
}

fn radii(value: &toml::Value, radius: &mut BTreeMap<String, u8>, problems: &mut Vec<String>) {
    let Some(table) = value.as_table() else {
        problems.push("[radius] 要是一张表".into());
        return;
    };
    for (key, value) in table {
        if !THEME_RADII.contains(&key.as_str()) {
            problems.push(format!("[radius] 里不认识的圆角 {}", shown(key)));
            continue;
        }
        match value
            .as_integer()
            .and_then(|pixels| u8::try_from(pixels).ok())
            .filter(|pixels| *pixels <= THEME_MAX_RADIUS)
        {
            Some(pixels) => {
                radius.insert(key.clone(), pixels);
            }
            None => {
                problems.push(format!("[radius] {key} 要是 0 到 {THEME_MAX_RADIUS} 的整数（像素）"))
            }
        }
    }
}

fn one_line(text: &str, max: usize) -> bool {
    !text.is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
}

/// An unknown key or a bad value as a problem repeats it: short, with control characters escaped.
fn shown(text: &str) -> String {
    text.chars().take(SHOWN_MAX_CHARS).flat_map(char::escape_debug).collect()
}
