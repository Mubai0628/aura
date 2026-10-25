use std::{
    collections::BTreeSet,
    fmt,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use aura_domain_common::{
    NetworkGrantId, RuntimeLeaseId, SandboxExecutionId, SecretNonce, UtcTimestamp,
};
use aura_permission_domain::{
    NetworkByteLimits, NetworkInspectionLevel, NormalizedOrigin, OpaqueTunnelGrant,
    OpaqueTunnelGrantRequest,
};
use aura_security_contract::NetworkDestinationClass;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{broadcast, watch},
    task::JoinHandle,
};
use zeroize::Zeroize;

use crate::{NetworkControlError, ProxyDnsResolver};

const MAX_PROXY_HEADER_BYTES: usize = 16 * 1024;
const MAX_TLS_CLIENT_HELLO_BYTES: usize = 64 * 1024;
const TLS_CLIENT_HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const TOKEN_BYTES: usize = 32;

#[derive(Clone, Debug)]
pub struct EgressProxyAuthorization {
    pub grant: OpaqueTunnelGrant,
}

impl EgressProxyAuthorization {
    pub async fn resolve(
        request: OpaqueTunnelGrantRequest,
        resolver: &ProxyDnsResolver,
    ) -> Result<Self, NetworkControlError> {
        let hostname = request.hostname.trim_end_matches('.').to_ascii_lowercase();
        if hostname.is_empty()
            || request.port == 0
            || request.max_connections == 0
            || request.expires_at <= UtcTimestamp::now()
        {
            return Err(NetworkControlError::GrantNotApplicable);
        }
        let public_only = BTreeSet::from([NetworkDestinationClass::Public]);
        let resolved = resolver.resolve_compliant_set(&hostname, &public_only).await?;
        let resolution_expires_at = if resolved.expires_at < request.expires_at {
            resolved.expires_at
        } else {
            request.expires_at
        };
        let authorization = Self {
            grant: OpaqueTunnelGrant {
                execution_id: request.execution_id,
                execution_lease_id: request.execution_lease_id,
                grant_id: request.grant_id,
                hostname,
                port: request.port,
                resolved_addresses: resolved.addresses.iter().map(ToString::to_string).collect(),
                resolution_expires_at,
                byte_limits: request.byte_limits,
                max_connections: request.max_connections,
                expires_at: request.expires_at,
            },
        };
        authorization.validate()?;
        Ok(authorization)
    }

    fn validate(&self) -> Result<(), NetworkControlError> {
        if self.grant.hostname.trim().is_empty()
            || self.grant.port == 0
            || self.grant.resolved_addresses.is_empty()
            || self.grant.resolved_addresses.iter().any(|address| {
                IpAddr::from_str(address).map_or(true, |address| {
                    crate::classify_ip(&address).as_slice() != [NetworkDestinationClass::Public]
                })
            })
            || self.grant.resolution_expires_at <= UtcTimestamp::now()
            || self.grant.resolution_expires_at > self.grant.expires_at
            || self.grant.max_connections == 0
            || self.expires_at() <= UtcTimestamp::now()
        {
            return Err(NetworkControlError::GrantNotApplicable);
        }
        Ok(())
    }

    const fn grant_id(&self) -> NetworkGrantId {
        self.grant.grant_id
    }

    const fn execution_id(&self) -> SandboxExecutionId {
        self.grant.execution_id
    }

    const fn execution_lease_id(&self) -> RuntimeLeaseId {
        self.grant.execution_lease_id
    }

    const fn byte_limits(&self) -> &NetworkByteLimits {
        &self.grant.byte_limits
    }

    const fn max_connections(&self) -> u32 {
        self.grant.max_connections
    }

    fn expires_at(&self) -> UtcTimestamp {
        self.grant.expires_at
    }

    fn origin(&self) -> NormalizedOrigin {
        NormalizedOrigin {
            scheme: "https".to_owned(),
            host: self.grant.hostname.clone(),
            port: self.grant.port,
        }
    }
}

pub struct ProxyAccessToken {
    bytes: [u8; TOKEN_BYTES],
}

impl ProxyAccessToken {
    #[must_use]
    pub fn authorization_header(&self) -> String {
        format!("Aura {}", lower_hex(&self.bytes))
    }

    #[must_use]
    pub fn into_secret_nonce(mut self) -> SecretNonce {
        let bytes = std::mem::replace(&mut self.bytes, [0_u8; TOKEN_BYTES]);
        SecretNonce::from_bytes(bytes)
    }
}

impl fmt::Debug for ProxyAccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProxyAccessToken([REDACTED])")
    }
}

impl Drop for ProxyAccessToken {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

#[derive(Clone)]
pub struct ProxyTokenAuthority {
    state: Arc<Mutex<ProxyTokenState>>,
}

#[derive(Default)]
struct ProxyTokenState {
    tickets: Vec<ProxyCredentialTicket>,
    revoked_grants: BTreeSet<NetworkGrantId>,
    revoked_executions: BTreeSet<SandboxExecutionId>,
    revoked_execution_leases: BTreeSet<RuntimeLeaseId>,
}

struct ProxyCredentialTicket {
    token_hash: [u8; 32],
    authorization: EgressProxyAuthorization,
    revocation: watch::Sender<bool>,
    connections_opened: u32,
    uploaded: Arc<AtomicU64>,
    downloaded: Arc<AtomicU64>,
}

struct AuthenticatedProxyAuthorization {
    authorization: EgressProxyAuthorization,
    revoked: watch::Receiver<bool>,
    uploaded: Arc<AtomicU64>,
    downloaded: Arc<AtomicU64>,
}

impl ProxyTokenAuthority {
    #[must_use]
    pub fn new() -> Self {
        Self { state: Arc::new(Mutex::new(ProxyTokenState::default())) }
    }

    pub fn issue(
        &self,
        authorization: EgressProxyAuthorization,
    ) -> Result<ProxyAccessToken, NetworkControlError> {
        authorization.validate()?;
        let mut token = [0_u8; TOKEN_BYTES];
        getrandom::fill(&mut token)
            .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
        let token_hash = token_hash(&token);
        let mut state = self
            .state
            .lock()
            .map_err(|_| NetworkControlError::Transport("proxy token store poisoned".to_owned()))?;
        if state.revoked_grants.contains(&authorization.grant_id())
            || state.revoked_executions.contains(&authorization.execution_id())
            || state.revoked_execution_leases.contains(&authorization.execution_lease_id())
        {
            token.zeroize();
            return Err(NetworkControlError::GrantNotApplicable);
        }
        let (revocation, _) = watch::channel(false);
        state.tickets.push(ProxyCredentialTicket {
            token_hash,
            authorization,
            revocation,
            connections_opened: 0,
            uploaded: Arc::new(AtomicU64::new(0)),
            downloaded: Arc::new(AtomicU64::new(0)),
        });
        Ok(ProxyAccessToken { bytes: token })
    }

    pub fn revoke_grant(&self, grant_id: NetworkGrantId) -> Result<(), NetworkControlError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| NetworkControlError::Transport("proxy token store poisoned".to_owned()))?;
        state.revoked_grants.insert(grant_id);
        for ticket in &state.tickets {
            if ticket.authorization.grant_id() == grant_id {
                ticket.revocation.send_replace(true);
            }
        }
        Ok(())
    }

    pub fn revoke_execution(
        &self,
        execution_id: SandboxExecutionId,
        execution_lease_id: RuntimeLeaseId,
    ) -> Result<(), NetworkControlError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| NetworkControlError::Transport("proxy token store poisoned".to_owned()))?;
        state.revoked_executions.insert(execution_id);
        state.revoked_execution_leases.insert(execution_lease_id);
        for ticket in &state.tickets {
            if ticket.authorization.execution_id() == execution_id
                || ticket.authorization.execution_lease_id() == execution_lease_id
            {
                ticket.revocation.send_replace(true);
            }
        }
        Ok(())
    }

    pub fn revoke_all(&self) -> Result<(), NetworkControlError> {
        let state = self
            .state
            .lock()
            .map_err(|_| NetworkControlError::Transport("proxy token store poisoned".to_owned()))?;
        for ticket in &state.tickets {
            ticket.revocation.send_replace(true);
        }
        Ok(())
    }

    fn authenticate(
        &self,
        authorization_header: &str,
    ) -> Result<AuthenticatedProxyAuthorization, NetworkControlError> {
        let encoded = authorization_header
            .strip_prefix("Aura ")
            .filter(|value| {
                !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_whitespace())
            })
            .ok_or(NetworkControlError::ProxyUnauthorized)?;
        let mut decoded = decode_token_hex(encoded)?;
        let hash = token_hash(&decoded);
        decoded.zeroize();
        let mut state = self
            .state
            .lock()
            .map_err(|_| NetworkControlError::Transport("proxy token store poisoned".to_owned()))?;
        let ticket = state
            .tickets
            .iter_mut()
            .find(|ticket| constant_time_hash_eq(&ticket.token_hash, &hash))
            .ok_or(NetworkControlError::ProxyUnauthorized)?;
        if *ticket.revocation.borrow()
            || ticket.authorization.expires_at() <= UtcTimestamp::now()
            || ticket.connections_opened >= ticket.authorization.max_connections()
        {
            return Err(NetworkControlError::ProxyUnauthorized);
        }
        ticket.connections_opened = ticket.connections_opened.saturating_add(1);
        Ok(AuthenticatedProxyAuthorization {
            authorization: ticket.authorization.clone(),
            revoked: ticket.revocation.subscribe(),
            uploaded: Arc::clone(&ticket.uploaded),
            downloaded: Arc::clone(&ticket.downloaded),
        })
    }
}

impl Default for ProxyTokenAuthority {
    fn default() -> Self {
        Self::new()
    }
}

fn token_hash(token: &[u8]) -> [u8; 32] {
    *blake3::hash(token).as_bytes()
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("writing to String is infallible");
    }
    value
}

fn decode_token_hex(value: &str) -> Result<[u8; TOKEN_BYTES], NetworkControlError> {
    if value.len() != TOKEN_BYTES * 2
        || !value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(NetworkControlError::ProxyUnauthorized);
    }
    let mut decoded = [0_u8; TOKEN_BYTES];
    for (index, output) in decoded.iter_mut().enumerate() {
        let offset = index * 2;
        *output = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| NetworkControlError::ProxyUnauthorized)?;
    }
    Ok(decoded)
}

fn constant_time_hash_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut difference = 0_u8;
    for index in 0..left.len() {
        difference |= left[index] ^ right[index];
    }
    difference == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProxyConnectionState {
    Started,
    Finished,
    Interrupted,
    Failed,
}

#[derive(Clone, Debug)]
pub struct ProxyConnectionEvent {
    pub grant_id: NetworkGrantId,
    pub execution_id: SandboxExecutionId,
    pub inspection_level: NetworkInspectionLevel,
    pub origin: NormalizedOrigin,
    pub state: ProxyConnectionState,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub error_code: Option<String>,
    pub occurred_at: UtcTimestamp,
}

pub struct EgressProxyServer {
    listener: TcpListener,
    authority: ProxyTokenAuthority,
    resolver: ProxyDnsResolver,
    events: broadcast::Sender<ProxyConnectionEvent>,
}

impl EgressProxyServer {
    pub async fn bind_loopback(
        authority: ProxyTokenAuthority,
    ) -> Result<Self, NetworkControlError> {
        Self::bind_loopback_with_resolver(authority, ProxyDnsResolver::default()).await
    }

    pub async fn bind_loopback_with_resolver(
        authority: ProxyTokenAuthority,
        resolver: ProxyDnsResolver,
    ) -> Result<Self, NetworkControlError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
        let (events, _) = broadcast::channel(128);
        Ok(Self { listener, authority, resolver, events })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, NetworkControlError> {
        self.listener
            .local_addr()
            .map_err(|error| NetworkControlError::Transport(error.to_string()))
    }

    pub fn spawn(self) -> Result<EgressProxyHandle, NetworkControlError> {
        let local_addr = self.local_addr()?;
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        let events = self.events.clone();
        let authority = self.authority.clone();
        let resolver = self.resolver.clone();
        let listener = self.listener;
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    changed = shutdown_rx.changed() => {
                        if changed.is_err() || *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    accepted = listener.accept() => {
                        let Ok((stream, peer)) = accepted else {
                            break;
                        };
                        if !peer.ip().is_loopback() {
                            continue;
                        }
                        let connection_authority = authority.clone();
                        let connection_events = events.clone();
                        let connection_resolver = resolver.clone();
                        tokio::spawn(async move {
                            let _ = handle_proxy_connection(
                                stream,
                                connection_authority,
                                connection_resolver,
                                connection_events,
                            ).await;
                        });
                    }
                }
            }
            let _ = authority.revoke_all();
        });
        Ok(EgressProxyHandle {
            local_addr,
            shutdown,
            authority: self.authority,
            events: self.events,
            task: Some(task),
        })
    }
}

pub struct EgressProxyHandle {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    authority: ProxyTokenAuthority,
    events: broadcast::Sender<ProxyConnectionEvent>,
    task: Option<JoinHandle<()>>,
}

impl EgressProxyHandle {
    #[must_use]
    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ProxyConnectionEvent> {
        self.events.subscribe()
    }

    pub async fn shutdown(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for EgressProxyHandle {
    fn drop(&mut self) {
        // Revoke synchronously before aborting the listener task so both
        // unconsumed tickets and active relay watch channels are invalidated
        // even when the proxy crashes or its owner is dropped abruptly.
        let _ = self.authority.revoke_all();
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn handle_proxy_connection(
    mut client: TcpStream,
    authority: ProxyTokenAuthority,
    resolver: ProxyDnsResolver,
    events: broadcast::Sender<ProxyConnectionEvent>,
) -> Result<(), NetworkControlError> {
    let (head, buffered_body) = match read_request_head(&mut client).await {
        Ok(value) => value,
        Err(error) => {
            let _ = write_status(&mut client, 400, "Bad Request").await;
            return Err(error);
        }
    };
    let request = match ParsedProxyRequest::parse(&head) {
        Ok(value) => value,
        Err(error) => {
            let _ = write_status(&mut client, 400, "Bad Request").await;
            return Err(error);
        }
    };
    let Some(proxy_authorization) = request.proxy_authorization.as_deref() else {
        let _ = write_proxy_auth_required(&mut client).await;
        return Err(NetworkControlError::ProxyUnauthorized);
    };
    let mut authenticated = match authority.authenticate(proxy_authorization) {
        Ok(value) => value,
        Err(error) => {
            let _ = write_proxy_auth_required(&mut client).await;
            return Err(error);
        }
    };
    if let Err(error) = validate_request_binding(&request, &authenticated.authorization) {
        let _ = write_status(&mut client, 403, "Forbidden").await;
        return Err(error);
    }
    let authorization = authenticated.authorization;
    let uploaded = authenticated.uploaded;
    let downloaded = authenticated.downloaded;
    let upload_before = uploaded.load(Ordering::Relaxed);
    let download_before = downloaded.load(Ordering::Relaxed);
    emit_proxy_event(&events, &authorization, ProxyConnectionState::Started, 0, 0, None);

    let remaining_nanos = authorization.expires_at().value().unix_timestamp_nanos()
        - UtcTimestamp::now().value().unix_timestamp_nanos();
    if remaining_nanos <= 0 {
        return Err(NetworkControlError::ProxyInterrupted);
    }
    let expires_in = Duration::from_nanos(u64::try_from(remaining_nanos).unwrap_or(u64::MAX));
    let transfer = relay_connect(
        client,
        buffered_body,
        &authorization,
        &resolver,
        uploaded.clone(),
        downloaded.clone(),
    );
    let result = tokio::select! {
        result = tokio::time::timeout(expires_in, transfer) => {
            result.map_err(|_| NetworkControlError::ProxyInterrupted)?
        }
        changed = authenticated.revoked.changed() => {
            let _ = changed;
            Err(NetworkControlError::ProxyInterrupted)
        }
    };
    let upload_bytes = uploaded.load(Ordering::Relaxed).saturating_sub(upload_before);
    let download_bytes = downloaded.load(Ordering::Relaxed).saturating_sub(download_before);
    match &result {
        Ok(()) => emit_proxy_event(
            &events,
            &authorization,
            ProxyConnectionState::Finished,
            upload_bytes,
            download_bytes,
            None,
        ),
        Err(error) => emit_proxy_event(
            &events,
            &authorization,
            if matches!(error, NetworkControlError::ProxyInterrupted) {
                ProxyConnectionState::Interrupted
            } else {
                ProxyConnectionState::Failed
            },
            upload_bytes,
            download_bytes,
            Some(error.to_string()),
        ),
    }
    result
}

fn emit_proxy_event(
    events: &broadcast::Sender<ProxyConnectionEvent>,
    authorization: &EgressProxyAuthorization,
    state: ProxyConnectionState,
    upload_bytes: u64,
    download_bytes: u64,
    error_code: Option<String>,
) {
    let _ = events.send(ProxyConnectionEvent {
        grant_id: authorization.grant_id(),
        execution_id: authorization.execution_id(),
        inspection_level: NetworkInspectionLevel::OpaqueTunnel,
        origin: authorization.origin(),
        state,
        upload_bytes,
        download_bytes,
        error_code,
        occurred_at: UtcTimestamp::now(),
    });
}

async fn relay_connect(
    mut client: TcpStream,
    buffered_body: Vec<u8>,
    authorization: &EgressProxyAuthorization,
    resolver: &ProxyDnsResolver,
    uploaded: Arc<AtomicU64>,
    downloaded: Arc<AtomicU64>,
) -> Result<(), NetworkControlError> {
    let mut remote = connect_to_checked_address(authorization, resolver).await?;
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\nConnection: close\r\n\r\n")
        .await
        .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
    let initial_tunnel_bytes = read_and_validate_tls_client_hello(
        &mut client,
        buffered_body,
        &authorization.grant.hostname,
    )
    .await?;
    let initial_length = u64::try_from(initial_tunnel_bytes.len()).unwrap_or(u64::MAX);
    if uploaded.load(Ordering::Relaxed).saturating_add(initial_length)
        > authorization.byte_limits().max_upload_bytes
    {
        return Err(NetworkControlError::UploadLimitExceeded);
    }
    if !initial_tunnel_bytes.is_empty() {
        remote
            .write_all(&initial_tunnel_bytes)
            .await
            .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
        uploaded.fetch_add(initial_length, Ordering::Relaxed);
    }
    let (mut client_reader, mut client_writer) = client.into_split();
    let (mut remote_reader, mut remote_writer) = remote.into_split();
    let upload = copy_limited(
        &mut client_reader,
        &mut remote_writer,
        authorization.byte_limits().max_upload_bytes,
        uploaded,
        CopyDirection::Upload,
    );
    let download = copy_limited(
        &mut remote_reader,
        &mut client_writer,
        authorization.byte_limits().max_download_bytes,
        downloaded,
        CopyDirection::Download,
    );
    let (upload_result, download_result) = tokio::join!(upload, download);
    upload_result?;
    download_result?;
    Ok(())
}

async fn read_and_validate_tls_client_hello(
    client: &mut TcpStream,
    mut bytes: Vec<u8>,
    expected_host: &str,
) -> Result<Vec<u8>, NetworkControlError> {
    loop {
        if let Some(server_name) = parse_tls_client_hello_server_name(&bytes)? {
            validate_tls_server_name(&server_name, expected_host)?;
            return Ok(bytes);
        }
        if bytes.len() >= MAX_TLS_CLIENT_HELLO_BYTES {
            return Err(NetworkControlError::ProxyProtocol);
        }
        let remaining = MAX_TLS_CLIENT_HELLO_BYTES - bytes.len();
        let mut buffer = vec![0_u8; remaining.min(4096)];
        let read = tokio::time::timeout(TLS_CLIENT_HELLO_TIMEOUT, client.read(&mut buffer))
            .await
            .map_err(|_| NetworkControlError::Timeout)?
            .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
        if read == 0 {
            return Err(NetworkControlError::ProxyProtocol);
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}

fn validate_tls_server_name(
    server_name: &str,
    expected_host: &str,
) -> Result<(), NetworkControlError> {
    if server_name.trim_end_matches('.').eq_ignore_ascii_case(expected_host.trim_end_matches('.')) {
        Ok(())
    } else {
        Err(NetworkControlError::TlsServerNameMismatch)
    }
}

fn parse_tls_client_hello_server_name(bytes: &[u8]) -> Result<Option<String>, NetworkControlError> {
    let mut record_offset = 0_usize;
    let mut handshake = Vec::new();
    loop {
        let Some(header) = bytes.get(record_offset..record_offset.saturating_add(5)) else {
            return Ok(None);
        };
        if header[0] != 22 || header[1] != 3 || header[2] > 4 {
            return Err(NetworkControlError::ProxyProtocol);
        }
        let record_length = usize::from(u16::from_be_bytes([header[3], header[4]]));
        if record_length == 0 || record_length > 16 * 1024 + 256 {
            return Err(NetworkControlError::ProxyProtocol);
        }
        let record_end = record_offset
            .checked_add(5)
            .and_then(|value| value.checked_add(record_length))
            .ok_or(NetworkControlError::ProxyProtocol)?;
        if record_end > MAX_TLS_CLIENT_HELLO_BYTES {
            return Err(NetworkControlError::ProxyProtocol);
        }
        let Some(payload) = bytes.get(record_offset + 5..record_end) else {
            return Ok(None);
        };
        handshake.extend_from_slice(payload);
        if handshake.len() >= 4 {
            if handshake[0] != 1 {
                return Err(NetworkControlError::ProxyProtocol);
            }
            let hello_length = (usize::from(handshake[1]) << 16)
                | (usize::from(handshake[2]) << 8)
                | usize::from(handshake[3]);
            let hello_end =
                4_usize.checked_add(hello_length).ok_or(NetworkControlError::ProxyProtocol)?;
            if hello_end > MAX_TLS_CLIENT_HELLO_BYTES {
                return Err(NetworkControlError::ProxyProtocol);
            }
            if handshake.len() >= hello_end {
                return parse_client_hello_server_name(&handshake[4..hello_end]).map(Some);
            }
        }
        record_offset = record_end;
    }
}

fn parse_client_hello_server_name(body: &[u8]) -> Result<String, NetworkControlError> {
    let mut cursor = 0_usize;
    take_tls_bytes(body, &mut cursor, 2 + 32)?;
    let session_id_length = usize::from(take_tls_u8(body, &mut cursor)?);
    take_tls_bytes(body, &mut cursor, session_id_length)?;
    let cipher_suites_length = usize::from(take_tls_u16(body, &mut cursor)?);
    if cipher_suites_length == 0 || cipher_suites_length % 2 != 0 {
        return Err(NetworkControlError::ProxyProtocol);
    }
    take_tls_bytes(body, &mut cursor, cipher_suites_length)?;
    let compression_methods_length = usize::from(take_tls_u8(body, &mut cursor)?);
    if compression_methods_length == 0 {
        return Err(NetworkControlError::ProxyProtocol);
    }
    take_tls_bytes(body, &mut cursor, compression_methods_length)?;
    let extensions_length = usize::from(take_tls_u16(body, &mut cursor)?);
    let extensions_end = cursor
        .checked_add(extensions_length)
        .filter(|end| *end == body.len())
        .ok_or(NetworkControlError::ProxyProtocol)?;
    let mut server_name = None;
    while cursor < extensions_end {
        let extension_type = take_tls_u16(body, &mut cursor)?;
        let extension_length = usize::from(take_tls_u16(body, &mut cursor)?);
        let extension = take_tls_bytes(body, &mut cursor, extension_length)?;
        if extension_type != 0 {
            continue;
        }
        if server_name.is_some() {
            return Err(NetworkControlError::ProxyProtocol);
        }
        let mut name_cursor = 0_usize;
        let list_length = usize::from(take_tls_u16(extension, &mut name_cursor)?);
        if list_length != extension.len().saturating_sub(name_cursor) {
            return Err(NetworkControlError::ProxyProtocol);
        }
        while name_cursor < extension.len() {
            let name_type = take_tls_u8(extension, &mut name_cursor)?;
            let name_length = usize::from(take_tls_u16(extension, &mut name_cursor)?);
            let name = take_tls_bytes(extension, &mut name_cursor, name_length)?;
            if name_type == 0 {
                if server_name.is_some()
                    || name.is_empty()
                    || !name.is_ascii()
                    || name.iter().any(|byte| byte.is_ascii_control() || *byte == 0)
                {
                    return Err(NetworkControlError::ProxyProtocol);
                }
                server_name = Some(
                    std::str::from_utf8(name)
                        .map_err(|_| NetworkControlError::ProxyProtocol)?
                        .to_owned(),
                );
            }
        }
    }
    server_name.ok_or(NetworkControlError::TlsServerNameMismatch)
}

fn take_tls_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, NetworkControlError> {
    let value = *bytes.get(*cursor).ok_or(NetworkControlError::ProxyProtocol)?;
    *cursor = cursor.checked_add(1).ok_or(NetworkControlError::ProxyProtocol)?;
    Ok(value)
}

fn take_tls_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, NetworkControlError> {
    let value = take_tls_bytes(bytes, cursor, 2)?;
    Ok(u16::from_be_bytes([value[0], value[1]]))
}

fn take_tls_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], NetworkControlError> {
    let end = cursor.checked_add(length).ok_or(NetworkControlError::ProxyProtocol)?;
    let value = bytes.get(*cursor..end).ok_or(NetworkControlError::ProxyProtocol)?;
    *cursor = end;
    Ok(value)
}

async fn connect_to_checked_address(
    authorization: &EgressProxyAuthorization,
    resolver: &ProxyDnsResolver,
) -> Result<TcpStream, NetworkControlError> {
    let mut last_error = None;
    let addresses = if authorization.grant.resolution_expires_at > UtcTimestamp::now() {
        authorization
            .grant
            .resolved_addresses
            .iter()
            .map(|value| {
                IpAddr::from_str(value).map_err(|_| NetworkControlError::DestinationBlocked)
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        resolver
            .resolve_compliant(
                &authorization.grant.hostname,
                &BTreeSet::from([NetworkDestinationClass::Public]),
            )
            .await?
    };
    for ip in addresses {
        if crate::classify_ip(&ip).as_slice() != [NetworkDestinationClass::Public] {
            return Err(NetworkControlError::DestinationBlocked);
        }
        let socket = SocketAddr::new(ip, authorization.grant.port);
        match TcpStream::connect(socket).await {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(NetworkControlError::Transport(
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "no checked destination address".to_owned()),
    ))
}

#[derive(Clone, Copy)]
enum CopyDirection {
    Upload,
    Download,
}

async fn copy_limited<R, W>(
    reader: &mut R,
    writer: &mut W,
    limit: u64,
    counter: Arc<AtomicU64>,
    direction: CopyDirection,
) -> Result<(), NetworkControlError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let current = counter.load(Ordering::Relaxed);
        if current >= limit {
            let mut extra = [0_u8; 1];
            let read = reader
                .read(&mut extra)
                .await
                .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
            if read == 0 {
                writer
                    .shutdown()
                    .await
                    .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
                return Ok(());
            }
            return Err(match direction {
                CopyDirection::Upload => NetworkControlError::UploadLimitExceeded,
                CopyDirection::Download => NetworkControlError::DownloadLimitExceeded,
            });
        }
        let wanted = usize::try_from((limit - current).min(buffer.len() as u64))
            .map_err(|_| NetworkControlError::DownloadLimitExceeded)?;
        let read = reader
            .read(&mut buffer[..wanted])
            .await
            .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
        if read == 0 {
            writer
                .shutdown()
                .await
                .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
            return Ok(());
        }
        writer
            .write_all(&buffer[..read])
            .await
            .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
        counter.fetch_add(u64::try_from(read).unwrap_or(u64::MAX), Ordering::Relaxed);
    }
}

struct ParsedProxyRequest {
    method: String,
    target: String,
    proxy_authorization: Option<String>,
}

impl ParsedProxyRequest {
    fn parse(head: &[u8]) -> Result<Self, NetworkControlError> {
        let text = std::str::from_utf8(head).map_err(|_| NetworkControlError::ProxyProtocol)?;
        if !text.is_ascii() || text.contains('\0') {
            return Err(NetworkControlError::ProxyProtocol);
        }
        let mut lines = text.split("\r\n");
        let request_line = lines.next().ok_or(NetworkControlError::ProxyProtocol)?;
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().ok_or(NetworkControlError::ProxyProtocol)?.to_owned();
        let target = request_parts.next().ok_or(NetworkControlError::ProxyProtocol)?.to_owned();
        let version = request_parts.next().ok_or(NetworkControlError::ProxyProtocol)?.to_owned();
        if request_parts.next().is_some()
            || !version.starts_with("HTTP/1.")
            || !method.bytes().all(|byte| byte.is_ascii_uppercase())
        {
            return Err(NetworkControlError::ProxyProtocol);
        }
        let mut proxy_authorization = None;
        for line in lines.filter(|line| !line.is_empty()) {
            if line.starts_with([' ', '\t']) {
                return Err(NetworkControlError::ProxyProtocol);
            }
            let (name, value) = line.split_once(':').ok_or(NetworkControlError::ProxyProtocol)?;
            let value = value.trim();
            if !valid_header_name(name) || value.bytes().any(|byte| byte.is_ascii_control()) {
                return Err(NetworkControlError::ProxyProtocol);
            }
            if name.eq_ignore_ascii_case("proxy-authorization") {
                if proxy_authorization.replace(value.to_owned()).is_some() {
                    return Err(NetworkControlError::ProxyProtocol);
                }
                continue;
            }
            if name.eq_ignore_ascii_case("proxy-aura-request-id") {
                return Err(NetworkControlError::ProxyProtocol);
            }
            if name.eq_ignore_ascii_case("content-length") {
                let parsed =
                    value.parse::<u64>().map_err(|_| NetworkControlError::ProxyProtocol)?;
                if parsed != 0 {
                    return Err(NetworkControlError::ProxyProtocol);
                }
            }
            if name.eq_ignore_ascii_case("transfer-encoding")
                || name.eq_ignore_ascii_case("upgrade")
            {
                return Err(NetworkControlError::ProxyProtocol);
            }
        }
        Ok(Self { method, target, proxy_authorization })
    }
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || byte == 0x60
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'|'
                        | b'~'
                )
        })
}

fn validate_request_binding(
    request: &ParsedProxyRequest,
    authorization: &EgressProxyAuthorization,
) -> Result<(), NetworkControlError> {
    if !request.method.eq_ignore_ascii_case("CONNECT") {
        return Err(NetworkControlError::GrantNotApplicable);
    }
    if !authority_matches(&request.target, &authorization.grant.hostname, authorization.grant.port)
    {
        return Err(NetworkControlError::GrantNotApplicable);
    }
    Ok(())
}

fn authority_matches(value: &str, expected_host: &str, expected_port: u16) -> bool {
    let candidate = format!("https://{value}/");
    url::Url::parse(&candidate).is_ok_and(|url| {
        url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.host_str().is_some_and(|host| host.eq_ignore_ascii_case(expected_host))
            && url.port_or_known_default() == Some(expected_port)
    })
}

async fn read_request_head(
    stream: &mut TcpStream,
) -> Result<(Vec<u8>, Vec<u8>), NetworkControlError> {
    let mut bytes = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    loop {
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer))
            .await
            .map_err(|_| NetworkControlError::Timeout)?
            .map_err(|error| NetworkControlError::Transport(error.to_string()))?;
        if read == 0 {
            return Err(NetworkControlError::ProxyProtocol);
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > MAX_PROXY_HEADER_BYTES {
            return Err(NetworkControlError::ProxyProtocol);
        }
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let body_start = position + 4;
            let body = bytes.split_off(body_start);
            return Ok((bytes, body));
        }
    }
}

async fn write_proxy_auth_required(stream: &mut TcpStream) -> Result<(), NetworkControlError> {
    stream
        .write_all(
            b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Aura\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        )
        .await
        .map_err(|error| NetworkControlError::Transport(error.to_string()))
}

async fn write_status(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
) -> Result<(), NetworkControlError> {
    stream
        .write_all(
            format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
                .as_bytes(),
        )
        .await
        .map_err(|error| NetworkControlError::Transport(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_permission_domain::OpaqueTunnelGrant;

    fn authorization(
        grant_id: NetworkGrantId,
        execution_id: SandboxExecutionId,
        execution_lease_id: RuntimeLeaseId,
        max_connections: u32,
    ) -> EgressProxyAuthorization {
        EgressProxyAuthorization {
            grant: OpaqueTunnelGrant {
                execution_id,
                execution_lease_id,
                grant_id,
                hostname: "example.test".to_owned(),
                port: 443,
                resolved_addresses: vec!["8.8.8.8".to_owned()],
                resolution_expires_at: UtcTimestamp::from_value(
                    UtcTimestamp::now().value() + time::Duration::minutes(1),
                ),
                byte_limits: NetworkByteLimits {
                    max_upload_bytes: 1024,
                    max_download_bytes: 1024,
                    max_decompressed_bytes: 2048,
                },
                max_connections,
                expires_at: UtcTimestamp::from_value(
                    UtcTimestamp::now().value() + time::Duration::minutes(1),
                ),
            },
        }
    }

    #[test]
    fn execution_token_reuses_physical_connections_only_within_frozen_limit() {
        let authority = ProxyTokenAuthority::new();
        let token = authority
            .issue(authorization(
                NetworkGrantId::new(),
                SandboxExecutionId::new(),
                RuntimeLeaseId::new(),
                2,
            ))
            .expect("issue token");
        assert_eq!(format!("{token:?}"), "ProxyAccessToken([REDACTED])");
        let header = token.authorization_header();
        assert_eq!(header.len(), "Aura ".len() + 64);
        authority.authenticate(&header).expect("first physical connection");
        authority.authenticate(&header).expect("necessary reconnect");
        assert!(matches!(
            authority.authenticate(&header),
            Err(NetworkControlError::ProxyUnauthorized)
        ));
    }

    #[test]
    fn opaque_tunnel_grants_reject_every_non_public_resolved_address() {
        let authority = ProxyTokenAuthority::new();
        for addresses in [
            vec!["127.0.0.1".to_owned()],
            vec!["10.0.0.1".to_owned()],
            vec!["169.254.1.1".to_owned()],
            vec!["169.254.169.254".to_owned()],
            vec!["192.0.2.1".to_owned()],
            vec!["8.8.8.8".to_owned(), "127.0.0.1".to_owned()],
            vec![],
        ] {
            let mut authorization = authorization(
                NetworkGrantId::new(),
                SandboxExecutionId::new(),
                RuntimeLeaseId::new(),
                1,
            );
            authorization.grant.resolved_addresses = addresses;
            assert!(matches!(
                authority.issue(authorization),
                Err(NetworkControlError::GrantNotApplicable)
            ));
        }
    }

    #[test]
    fn revoking_grant_invalidates_future_connections() {
        let authority = ProxyTokenAuthority::new();
        let grant_id = NetworkGrantId::new();
        let token = authority
            .issue(authorization(grant_id, SandboxExecutionId::new(), RuntimeLeaseId::new(), 2))
            .expect("issue token");
        authority.revoke_grant(grant_id).expect("revoke");
        assert!(matches!(
            authority.authenticate(&token.authorization_header()),
            Err(NetworkControlError::ProxyUnauthorized)
        ));
    }

    #[test]
    fn ending_execution_lease_invalidates_future_connections() {
        let authority = ProxyTokenAuthority::new();
        let execution_id = SandboxExecutionId::new();
        let execution_lease_id = RuntimeLeaseId::new();
        let token = authority
            .issue(authorization(NetworkGrantId::new(), execution_id, execution_lease_id, 2))
            .expect("issue token");
        authority.revoke_execution(execution_id, execution_lease_id).expect("revoke execution");
        assert!(matches!(
            authority.authenticate(&token.authorization_header()),
            Err(NetworkControlError::ProxyUnauthorized)
        ));
    }

    #[tokio::test]
    async fn proxy_rejects_connections_without_authentication() {
        let authority = ProxyTokenAuthority::new();
        let server = EgressProxyServer::bind_loopback(authority).await.expect("bind proxy");
        let handle = server.spawn().expect("spawn proxy");
        let mut stream = TcpStream::connect(handle.local_addr()).await.expect("connect proxy");
        stream
            .write_all(b"GET http://example.test/ HTTP/1.1\r\nHost: example.test\r\n\r\n")
            .await
            .expect("write request");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read response");
        assert!(response.starts_with(b"HTTP/1.1 407"));
        handle.shutdown().await;
    }

    #[test]
    fn https_tunnel_requires_exact_authorized_tls_server_name() {
        let hello = tls_client_hello("cdn.example.test");
        let parsed = parse_tls_client_hello_server_name(&hello)
            .expect("valid ClientHello")
            .expect("complete ClientHello");
        assert_eq!(parsed, "cdn.example.test");
        validate_tls_server_name(&parsed, "CDN.EXAMPLE.TEST.").expect("same normalized host");
        assert!(matches!(
            validate_tls_server_name(&parsed, "other.example.test"),
            Err(NetworkControlError::TlsServerNameMismatch)
        ));
        assert_eq!(
            parse_tls_client_hello_server_name(&hello[..hello.len() - 1])
                .expect("incomplete ClientHello"),
            None
        );
    }

    #[test]
    fn https_tunnel_accepts_fragmented_client_hello_records_but_rejects_missing_sni() {
        let hello = tls_client_hello("example.test");
        let handshake = &hello[5..];
        let split = 13;
        let mut fragmented = tls_record(&handshake[..split]);
        fragmented.extend_from_slice(&tls_record(&handshake[split..]));
        assert_eq!(
            parse_tls_client_hello_server_name(&fragmented)
                .expect("fragmented ClientHello")
                .as_deref(),
            Some("example.test")
        );

        let missing_sni = tls_client_hello_with_extensions(&[]);
        assert!(matches!(
            parse_tls_client_hello_server_name(&missing_sni),
            Err(NetworkControlError::TlsServerNameMismatch)
        ));
    }

    fn tls_client_hello(server_name: &str) -> Vec<u8> {
        let name = server_name.as_bytes();
        let mut server_name_list = Vec::new();
        server_name_list.push(0);
        server_name_list.extend_from_slice(
            &u16::try_from(name.len()).expect("fixture host length").to_be_bytes(),
        );
        server_name_list.extend_from_slice(name);
        let mut server_name_extension = Vec::new();
        server_name_extension.extend_from_slice(
            &u16::try_from(server_name_list.len())
                .expect("fixture server name list length")
                .to_be_bytes(),
        );
        server_name_extension.extend_from_slice(&server_name_list);
        let mut extensions = Vec::new();
        extensions.extend_from_slice(&0_u16.to_be_bytes());
        extensions.extend_from_slice(
            &u16::try_from(server_name_extension.len())
                .expect("fixture SNI extension length")
                .to_be_bytes(),
        );
        extensions.extend_from_slice(&server_name_extension);
        tls_client_hello_with_extensions(&extensions)
    }

    fn tls_client_hello_with_extensions(extensions: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&[3, 3]);
        body.extend_from_slice(&[0x42; 32]);
        body.push(0);
        body.extend_from_slice(&2_u16.to_be_bytes());
        body.extend_from_slice(&0x1301_u16.to_be_bytes());
        body.push(1);
        body.push(0);
        body.extend_from_slice(
            &u16::try_from(extensions.len()).expect("fixture extensions length").to_be_bytes(),
        );
        body.extend_from_slice(extensions);
        let body_length = u32::try_from(body.len()).expect("fixture ClientHello length");
        let mut handshake = vec![
            1,
            u8::try_from((body_length >> 16) & 0xff).expect("high length byte"),
            u8::try_from((body_length >> 8) & 0xff).expect("middle length byte"),
            u8::try_from(body_length & 0xff).expect("low length byte"),
        ];
        handshake.extend_from_slice(&body);
        tls_record(&handshake)
    }

    fn tls_record(payload: &[u8]) -> Vec<u8> {
        let mut record = vec![22, 3, 1];
        record.extend_from_slice(
            &u16::try_from(payload.len()).expect("fixture TLS record length").to_be_bytes(),
        );
        record.extend_from_slice(payload);
        record
    }
}
