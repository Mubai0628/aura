#![forbid(unsafe_code)]

mod client;
mod discovery;
mod match_model;
mod parser;
mod persistence;
mod projection;

pub use client::{
    CatalogCache, CatalogCacheRecord, CatalogConditionalRequest, CatalogFetchResponse,
    CatalogRequestAuthorizer, CatalogSource, CoreGatewayCatalogSource,
    CoreGatewayEndpointDiscoverySource, EndpointDiscoveryFetchResponse,
    EndpointDiscoveryRequestAuthorizer, EndpointDiscoverySource, InMemoryCatalogCache,
    ProviderCatalogClient, ProviderCatalogService, RefreshOwnership,
};
pub use discovery::{
    ModelDiscoveryAccumulator, ModelDiscoveryPage, ModelDiscoveryPageAction, discovery_page_url,
    parse_model_discovery_page,
};
pub use match_model::{match_catalog_model, official_catalog_provider};
pub use parser::parse_models_dev_catalog;
pub use persistence::PersistenceCatalogCache;
pub use projection::{
    ProviderConfigurationPolicy, project_provider_cards, project_provider_model_page,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog source failed: {0}")]
    Source(String),
    #[error("catalog request used the wrong P04 caller kind")]
    WrongCaller,
    #[error("endpoint discovery request must be an empty GET request")]
    InvalidDiscoveryRequest,
    #[error("models.dev returned HTTP status {0}")]
    HttpStatus(u16),
    #[error("models.dev returned not-modified without a cached snapshot")]
    NotModifiedWithoutCache,
    #[error("models.dev catalog JSON is invalid: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("models.dev catalog is invalid: {0}")]
    Invalid(String),
    #[error("discovery.invalid_response: {0}")]
    DiscoveryInvalidResponse(String),
    #[error("discovery.limit_exceeded")]
    DiscoveryLimitExceeded,
    #[error("catalog cache failed: {0}")]
    Cache(String),
    #[error("catalog state lock was poisoned")]
    LockPoisoned,
    #[error("provider catalog refresh is already in progress")]
    RefreshInProgress,
    #[error("provider catalog revision overflow")]
    RevisionOverflow,
    #[error("multiple local Provider objects map to models.dev provider {0}")]
    AmbiguousLocalProvider(String),
    #[error("catalog is not available")]
    Unavailable,
    #[error("catalog provider was not found: {0}")]
    ProviderNotFound(String),
    #[error("catalog revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("catalog page limit must be in 1..=100")]
    InvalidPageLimit,
    #[error("a catalog continuation requires expected_catalog_revision")]
    MissingContinuationRevision,
    #[error("catalog continuation model was not found: {0}")]
    InvalidContinuation(String),
    #[error(transparent)]
    Network(#[from] aura_network_control::NetworkControlError),
}
