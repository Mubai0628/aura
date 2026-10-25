use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HttpRequestBody {
    Text {
        text: String,
        #[serde(default)]
        content_type: Option<String>,
    },
    Json {
        value: serde_json::Value,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpRequestInput {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: Vec<HttpHeader>,
    #[serde(default)]
    pub body: Option<HttpRequestBody>,
    /// Selects an existing credential slot configured by the user for this exact tool.
    #[serde(default)]
    pub credential: Option<String>,
}
fn default_method() -> String {
    "GET".into()
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpDownloadInput {
    pub url: String,
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
    /// For a site that serves a file only to its own pages: User-Agent, Referer, Accept and
    /// Accept-Language only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<HttpHeader>,
}
