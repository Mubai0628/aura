//! Dedicated-tool authority uses the shared Gateway's DNS, limits and credential boundary.
mod endpoints;
mod errors;
mod grant;
mod material;
pub(crate) use endpoints::check_destination;
pub use endpoints::{
    ControlEndpointRegistration, register_control_endpoint, registered_control_endpoints,
};
pub use errors::safe_transport_code;
pub use grant::OrdinaryToolRequestGrant;
pub use material::{ordinary_request_hash, safe_url};
