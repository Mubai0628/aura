use crate::NetworkControlError;
use aura_domain_common::RuntimeInstanceId;
use aura_security_contract::ResolvedNetworkDestination;
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, OnceLock},
};

type Endpoints = Mutex<BTreeMap<(RuntimeInstanceId, SocketAddr), usize>>;
fn endpoints() -> &'static Endpoints {
    static VALUE: OnceLock<Endpoints> = OnceLock::new();
    VALUE.get_or_init(Endpoints::default)
}

/// A live Core-observed listener registration. The final owner unregisters exactly its own
/// runtime/listener; an unrelated process on the same numeric port is never classified by port alone.
#[derive(Clone)]
pub struct ControlEndpointRegistration(Arc<Registration>);
struct Registration {
    runtime: RuntimeInstanceId,
    endpoint: SocketAddr,
}

pub fn registered_control_endpoints() -> Result<Vec<SocketAddr>, NetworkControlError> {
    let entries = endpoints().lock().map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?;
    Ok(entries.keys().map(|(_, endpoint)| *endpoint).collect())
}

pub fn register_control_endpoint(
    runtime: RuntimeInstanceId,
    endpoint: SocketAddr,
) -> Result<ControlEndpointRegistration, NetworkControlError> {
    let endpoint = canonical_endpoint(endpoint);
    let mut entries =
        endpoints().lock().map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?;
    *entries.entry((runtime, endpoint)).or_default() += 1;
    Ok(ControlEndpointRegistration(Arc::new(Registration { runtime, endpoint })))
}

impl ControlEndpointRegistration {
    pub fn endpoint(&self) -> SocketAddr {
        self.0.endpoint
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut entries) = endpoints().lock() {
            let key = (self.runtime, self.endpoint);
            if let Some(count) = entries.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    entries.remove(&key);
                }
            }
        }
    }
}

pub(crate) fn check_destination(
    destination: &ResolvedNetworkDestination,
) -> Result<(), NetworkControlError> {
    let entries = endpoints().lock().map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?;
    for address in &destination.addresses {
        let ip: IpAddr = address.parse().map_err(|_| NetworkControlError::DestinationBlocked)?;
        let endpoint = canonical_endpoint(SocketAddr::new(ip, destination.port));
        if entries.keys().any(|(_, held)| *held == endpoint) {
            return Err(NetworkControlError::ControlEndpointBlocked);
        }
    }
    Ok(())
}

fn canonical_endpoint(endpoint: SocketAddr) -> SocketAddr {
    let ip = match endpoint.ip() {
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(IpAddr::V6(ip), IpAddr::V4),
        ip => ip,
    };
    SocketAddr::new(ip, endpoint.port())
}
