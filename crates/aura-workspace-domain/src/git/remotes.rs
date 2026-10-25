//! Native remote operations. Inputs select configured names, never model-supplied URLs.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(extend("type" = "object"))]
pub enum GitSyncInput {
    Fetch {
        #[serde(default)]
        remote: Option<String>,
    },
    Pull {
        #[serde(default)]
        remote: Option<String>,
        #[serde(default)]
        branch: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitPushInput {
    #[serde(default)]
    pub remote: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub remote_branch: Option<String>,
    #[serde(default)]
    pub set_upstream: bool,
    /// Full expected remote object ID. An exact lease, never a bare force or tracking-ref lease.
    #[serde(default)]
    pub force_with_lease: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitRefUpdateV1 {
    pub reference: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub result: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitRemoteResultV1 {
    pub kind: String,
    pub remote: String,
    /// Normalized display and consent URL; has no userinfo, query or fragment.
    pub remote_url: String,
    pub local_branch: Option<String>,
    pub remote_branch: Option<String>,
    pub refs: Vec<GitRefUpdateV1>,
    pub status: super::GitStatusResultV1,
    pub execution_notice: String,
}
