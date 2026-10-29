//! Why a connection to a request's target failed, as one reason Core can explain to the model.
//! The library's and the system's own text never leaves this module: it can carry the URL, the
//! resolver's answer or the machine's configuration.
use std::io::ErrorKind;

/// The reasons `NetworkControlError::Transport` carries for a target that could not be reached.
pub const REFUSED: &str = "connection refused";
pub const HOST_NOT_FOUND: &str = "host not found";
pub const TLS_FAILED: &str = "TLS failed";
pub const RESET: &str = "connection reset";
pub const CONNECT_FAILED: &str = "connection failed";
/// Anything else: the request may have reached the target.
pub const REQUEST_FAILED: &str = "request failed";

/// The model-safe code for a transport reason.
#[must_use]
pub fn code(reason: &str) -> &'static str {
    match reason {
        REFUSED => "network.connection_refused",
        HOST_NOT_FOUND => "network.host_not_found",
        TLS_FAILED => "network.tls_failed",
        RESET => "network.connection_reset",
        CONNECT_FAILED => "network.connection_failed",
        _ => "network.transport_failed",
    }
}

/// The reason of an HTTP client failure, from the error kinds in its source chain.
pub(crate) fn of_request(error: &reqwest::Error) -> &'static str {
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>()
            && let Some(reason) = of_io(io)
        {
            return reason;
        }
        if cause.downcast_ref::<tokio_rustls::rustls::Error>().is_some() {
            return TLS_FAILED;
        }
        source = cause.source();
    }
    if error.is_connect() { CONNECT_FAILED } else { REQUEST_FAILED }
}

fn of_io(error: &std::io::Error) -> Option<&'static str> {
    // WSAECONNREFUSED; WSAHOST_NOT_FOUND, WSANO_DATA; WSAECONNRESET, WSAECONNABORTED.
    match error.raw_os_error() {
        Some(10061) => return Some(REFUSED),
        Some(11001 | 11004) => return Some(HOST_NOT_FOUND),
        Some(10054 | 10053) => return Some(RESET),
        _ => {}
    }
    match error.kind() {
        ErrorKind::ConnectionRefused => Some(REFUSED),
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => Some(RESET),
        _ => error
            .get_ref()
            .is_some_and(|inner| inner.downcast_ref::<tokio_rustls::rustls::Error>().is_some())
            .then_some(TLS_FAILED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_errors_become_reasons_and_reasons_become_codes() {
        let refused = std::io::Error::from_raw_os_error(10061);
        assert_eq!(of_io(&refused), Some(REFUSED));
        assert_eq!(of_io(&std::io::Error::from(ErrorKind::ConnectionRefused)), Some(REFUSED));
        assert_eq!(of_io(&std::io::Error::from_raw_os_error(11001)), Some(HOST_NOT_FOUND));
        assert_eq!(of_io(&std::io::Error::from(ErrorKind::ConnectionReset)), Some(RESET));
        let tls =
            std::io::Error::new(ErrorKind::InvalidData, tokio_rustls::rustls::Error::DecryptError);
        assert_eq!(of_io(&tls), Some(TLS_FAILED));
        assert_eq!(of_io(&std::io::Error::from(ErrorKind::PermissionDenied)), None);
        assert_eq!(code(REFUSED), "network.connection_refused");
        assert_eq!(code(HOST_NOT_FOUND), "network.host_not_found");
        assert_eq!(code("No such host is known. (os error 11001)"), "network.transport_failed");
    }
}
