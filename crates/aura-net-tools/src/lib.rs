//! Ordinary HTTP tool business adapters. Core owns authority; the gateway owns transport.
mod request;
mod response;
pub use request::{download_request, http_request, request_url, web_request, web_service_request};
pub use response::http_result;
