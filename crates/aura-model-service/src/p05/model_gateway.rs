//! The gateway model requests leave through: network grants and provider credentials from the
//! store, and for each request the proxy the settings name (Aura's own while switched on, else the
//! system's).
use std::sync::Arc;

use aura_config::SettingsFacade;
use aura_network_control::{
    CoreHttpClientGateway, NetworkControlOptions, PersistenceNetworkGrantStore,
};
use aura_persistence::PersistenceCoordinator;

use crate::p05_credential::PersistentProviderCredentialResolver;

pub(super) fn open(
    persistence: &Arc<PersistenceCoordinator>,
    settings: &Arc<SettingsFacade>,
) -> Arc<CoreHttpClientGateway> {
    let network_grants = Arc::new(PersistenceNetworkGrantStore::new(persistence.as_ref().clone()));
    let provider_credentials =
        Arc::new(PersistentProviderCredentialResolver::new(Arc::clone(persistence)));
    let settings = Arc::clone(settings);
    Arc::new(
        CoreHttpClientGateway::new(network_grants, None, NetworkControlOptions::default())
            .with_provider_credentials(provider_credentials)
            .with_model_proxy(Arc::new(move || settings.model_proxy())),
    )
}
