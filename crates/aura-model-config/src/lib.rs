#![forbid(unsafe_code)]

mod context;
mod credential_status;
mod crud;
mod routing;
pub use routing::{connection_order, next_connection};
mod discovery;
mod fingerprint;
mod request_options;
mod service;
mod validation;

pub use context::{
    CONTEXT_WINDOW_AUTOMATIC_CANDIDATES, apply_runtime_context_window_candidates,
    apply_runtime_context_window_correction, next_context_window_candidate,
    reset_runtime_context_window_correction, set_historical_snapshot_max_tokens,
    set_user_context_window,
};
pub use credential_status::{
    CredentialAvailability, CredentialSlotStatusFacts, ModelCredentialStatusError,
    derive_endpoint_credential_status,
};
pub use crud::{
    LearnChatOutputTokenFieldAliasResult, UpdateModelProfileInput, create_endpoint,
    create_provider, delete_endpoint, delete_model_profile, delete_provider,
    learn_chat_output_token_field_alias, rename_model_profile, rename_provider,
    reset_endpoint_compatibility, set_model_subagent_availability, set_provider_connections,
    set_provider_current_models_subagent_availability, update_endpoint, update_model_profile,
};
pub use discovery::{
    add_manual_model, apply_catalog_update, create_model_from_catalog, merge_discovery_results,
};
pub use fingerprint::{
    ModelUrlError, credential_binding_fingerprint, credential_ref_for_credential,
    endpoint_fingerprint, model_credential_final_target, model_name_hash, normalize_base_url,
    resolve_api_url, resolve_models_url,
};
pub use service::{
    CreateModelExecutionSnapshot, ModelConfigError, ModelConfigService, ModelConfigSnapshot,
    ModelExecutionCredentialBinding, PreparedModelConfigMutation,
};
pub use validation::{ModelConfigValidationError, ModelConfigValidator, validate_model_registry};
