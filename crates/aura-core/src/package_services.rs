//! The package service is aura-package-service's. Core keeps the test support that installs
//! packages through a started Core's services; it reads the service's names from here.
pub(crate) use aura_package_service::package_services::*;
#[cfg(test)]
pub(crate) mod test_support;
