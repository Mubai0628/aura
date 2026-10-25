use aura_domain_common::{Revision, RuntimeInstanceId};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{ProjectionKind, ProjectionRouteError, ProjectionScope};

#[derive(Clone, Copy, Debug, JsonSchema, PartialEq, Serialize)]
#[schemars(schema_with = "crate::schema::projection_subscription_schema")]
#[serde(deny_unknown_fields)]
pub struct ProjectionSubscription {
    pub projection_kind: ProjectionKind,
    pub projection_scope: ProjectionScope,
    pub known_runtime_instance_id: Option<RuntimeInstanceId>,
    pub after_revision: Option<Revision>,
}

impl ProjectionSubscription {
    pub const fn validate(self) -> Result<(), ProjectionRouteError> {
        self.projection_kind.validate_scope(self.projection_scope)
    }
}

impl<'de> Deserialize<'de> for ProjectionSubscription {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSubscription {
            projection_kind: ProjectionKind,
            projection_scope: ProjectionScope,
            known_runtime_instance_id: Option<RuntimeInstanceId>,
            after_revision: Option<Revision>,
        }

        let wire = WireSubscription::deserialize(deserializer)?;
        let subscription = Self {
            projection_kind: wire.projection_kind,
            projection_scope: wire.projection_scope,
            known_runtime_instance_id: wire.known_runtime_instance_id,
            after_revision: wire.after_revision,
        };
        subscription.validate().map_err(D::Error::custom)?;
        Ok(subscription)
    }
}

#[derive(Clone, Copy, Debug, JsonSchema, PartialEq, Serialize)]
#[schemars(schema_with = "crate::schema::projection_snapshot_request_schema")]
#[serde(deny_unknown_fields)]
pub struct ProjectionSnapshotRequest {
    pub projection_kind: ProjectionKind,
    pub projection_scope: ProjectionScope,
    pub known_runtime_instance_id: Option<RuntimeInstanceId>,
}

#[derive(Clone, Copy, Debug, JsonSchema, PartialEq, Serialize)]
#[schemars(schema_with = "crate::schema::projection_unsubscribe_request_schema")]
#[serde(deny_unknown_fields)]
pub struct ProjectionUnsubscribeRequest {
    pub projection_kind: ProjectionKind,
    pub projection_scope: ProjectionScope,
}

impl ProjectionUnsubscribeRequest {
    pub const fn validate(self) -> Result<(), ProjectionRouteError> {
        self.projection_kind.validate_scope(self.projection_scope)
    }
}

impl<'de> Deserialize<'de> for ProjectionUnsubscribeRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireRequest {
            projection_kind: ProjectionKind,
            projection_scope: ProjectionScope,
        }

        let wire = WireRequest::deserialize(deserializer)?;
        let request =
            Self { projection_kind: wire.projection_kind, projection_scope: wire.projection_scope };
        request.validate().map_err(D::Error::custom)?;
        Ok(request)
    }
}

impl ProjectionSnapshotRequest {
    pub const fn validate(self) -> Result<(), ProjectionRouteError> {
        self.projection_kind.validate_scope(self.projection_scope)
    }
}

impl<'de> Deserialize<'de> for ProjectionSnapshotRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSnapshotRequest {
            projection_kind: ProjectionKind,
            projection_scope: ProjectionScope,
            known_runtime_instance_id: Option<RuntimeInstanceId>,
        }

        let wire = WireSnapshotRequest::deserialize(deserializer)?;
        let request = Self {
            projection_kind: wire.projection_kind,
            projection_scope: wire.projection_scope,
            known_runtime_instance_id: wire.known_runtime_instance_id,
        };
        request.validate().map_err(D::Error::custom)?;
        Ok(request)
    }
}
