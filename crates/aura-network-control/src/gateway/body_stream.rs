//! A response body read as it arrives: wire and decompressed byte limits, the download
//! rate, and the content codings the gateway accepts.
use super::*;

pub struct GatewayBodyStream {
    reader: Pin<Box<dyn AsyncRead + Send>>,
    content_coding: ResponseContentCoding,
    max_decompressed_bytes: u64,
    decompressed_bytes: u64,
    /// For a request that carried an injected credential: every chunk is scanned for the
    /// credential before the caller sees it.
    reflection: Option<(CredentialReflectionScannerV1, CredentialReflectionSurfaceV1, Vec<u8>)>,
}

impl GatewayBodyStream {
    pub(super) fn new(
        response: reqwest::Response,
        headers: &[(String, String)],
        rate_permit: crate::grant::RateLimitPermit,
        max_download_bytes: u64,
        max_decompressed_bytes: u64,
    ) -> Result<Self, NetworkControlError> {
        let coding = ResponseContentCoding::parse(headers)?;
        let stream = WireLimitedResponseStream {
            inner: Box::pin(response.bytes_stream()),
            rate_permit,
            max_wire_bytes: max_download_bytes,
            wire_bytes: 0,
        };
        let reader = BufReader::new(StreamReader::new(stream));
        let reader: Pin<Box<dyn AsyncRead + Send>> = match coding {
            ResponseContentCoding::Identity => Box::pin(reader),
            ResponseContentCoding::Gzip => Box::pin(GzipDecoder::new(reader)),
            ResponseContentCoding::Brotli => Box::pin(BrotliDecoder::new(reader)),
            ResponseContentCoding::Zstd => Box::pin(ZstdDecoder::new(reader)),
            ResponseContentCoding::Zlib => Box::pin(ZlibDecoder::new(reader)),
        };
        Ok(Self {
            reader,
            content_coding: coding,
            max_decompressed_bytes,
            decompressed_bytes: 0,
            reflection: None,
        })
    }

    /// Scans every chunk for the injected credential (`surface` names the final or the error
    /// body); a reflected credential ends the stream with the detection.
    pub(super) fn scanned(
        mut self,
        scanner: CredentialReflectionScannerV1,
        surface: CredentialReflectionSurfaceV1,
    ) -> Self {
        self.reflection = Some((scanner, surface, Vec::new()));
        self
    }

    pub async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, NetworkControlError> {
        let mut chunk = vec![0_u8; 16 * 1024];
        let read = match self.reader.read(&mut chunk).await {
            Ok(read) => read,
            Err(error) => return Err(map_body_read_error(&error, self.content_coding)),
        };
        if read == 0 {
            return Ok(None);
        }
        chunk.truncate(read);
        let chunk_length = u64::try_from(read).unwrap_or(u64::MAX);
        let next_length = self.decompressed_bytes.saturating_add(chunk_length);
        if next_length > self.max_decompressed_bytes {
            return Err(NetworkControlError::ResponseTooLarge);
        }
        self.decompressed_bytes = next_length;
        if let Some((scanner, surface, tail)) = self.reflection.as_mut() {
            scanner.scan_body_chunk(tail, &chunk, *surface)?;
        }
        Ok(Some(chunk))
    }

    #[must_use]
    pub const fn bytes_received(&self) -> u64 {
        self.decompressed_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResponseContentCoding {
    Identity,
    Gzip,
    Brotli,
    Zstd,
    Zlib,
}

impl ResponseContentCoding {
    pub(super) fn parse(headers: &[(String, String)]) -> Result<Self, NetworkControlError> {
        let codings = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
            .flat_map(|(_, value)| value.split(','))
            .map(|value| value.trim_matches([' ', '\t']).to_ascii_lowercase())
            .collect::<Vec<_>>();
        match codings.as_slice() {
            [] => Ok(Self::Identity),
            [coding] if coding == "identity" => Ok(Self::Identity),
            [coding] if coding == "gzip" => Ok(Self::Gzip),
            [coding] if coding == "br" => Ok(Self::Brotli),
            [coding] if coding == "zstd" => Ok(Self::Zstd),
            [coding] if coding == "deflate" => Ok(Self::Zlib),
            _ => Err(NetworkControlError::ContentEncodingUnsupported),
        }
    }
}

pub(super) type BoxedResponseByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

pub(super) struct WireLimitedResponseStream {
    inner: BoxedResponseByteStream,
    rate_permit: crate::grant::RateLimitPermit,
    max_wire_bytes: u64,
    wire_bytes: u64,
}

impl Stream for WireLimitedResponseStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        match this.inner.as_mut().poll_next(context) {
            Poll::Ready(Some(Ok(bytes))) => {
                let byte_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
                let next_length = this.wire_bytes.saturating_add(byte_length);
                if next_length > this.max_wire_bytes {
                    return Poll::Ready(Some(Err(io::Error::other(
                        WireBodyReadError::ResponseTooLarge,
                    ))));
                }
                if let Err(error) = this.rate_permit.charge_download(byte_length) {
                    return Poll::Ready(Some(Err(io::Error::other(
                        WireBodyReadError::from_network(error),
                    ))));
                }
                this.wire_bytes = next_length;
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(error))) => {
                Poll::Ready(Some(Err(io::Error::other(if error.is_timeout() {
                    WireBodyReadError::Timeout
                } else {
                    WireBodyReadError::Transport
                }))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Debug)]
pub(super) enum WireBodyReadError {
    Timeout,
    RateLimited,
    ResponseTooLarge,
    Transport,
}

impl WireBodyReadError {
    fn from_network(error: NetworkControlError) -> Self {
        match error {
            NetworkControlError::RateLimited => Self::RateLimited,
            NetworkControlError::Timeout => Self::Timeout,
            NetworkControlError::DownloadLimitExceeded | NetworkControlError::ResponseTooLarge => {
                Self::ResponseTooLarge
            }
            _ => Self::Transport,
        }
    }
}

impl fmt::Display for WireBodyReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Timeout => "provider response body timed out",
            Self::RateLimited => "provider response exceeded the download rate limit",
            Self::ResponseTooLarge => "provider response exceeded its wire byte limit",
            Self::Transport => "provider response body transport failed",
        })
    }
}

impl StdError for WireBodyReadError {}

pub(super) fn map_body_read_error(
    error: &io::Error,
    content_coding: ResponseContentCoding,
) -> NetworkControlError {
    if let Some(failure) = classify_wire_body_read_error(error) {
        return failure;
    }
    // An identity body has no codec capable of producing a decompression
    // error. Some HTTP stacks collapse their source error while adapting a
    // failed body stream into `AsyncRead`; preserve the transport category in
    // that case so retry classification remains correct. For an encoded body,
    // an unclassified reader failure means the advertised coding could not be
    // decoded to a complete stream and therefore follows the frozen
    // `transport.decompression_failed` contract.
    if content_coding == ResponseContentCoding::Identity {
        NetworkControlError::Transport("response body read failed".to_owned())
    } else {
        NetworkControlError::DecompressionFailed
    }
}

pub(super) fn classify_wire_body_read_error(error: &io::Error) -> Option<NetworkControlError> {
    if let Some(failure) =
        error.get_ref().and_then(|inner| inner.downcast_ref::<WireBodyReadError>())
    {
        return Some(map_wire_body_read_error(failure));
    }
    let mut source = error.source();
    while let Some(current) = source {
        if let Some(failure) = current.downcast_ref::<WireBodyReadError>() {
            return Some(map_wire_body_read_error(failure));
        }
        source = current.source();
    }
    None
}

pub(super) fn map_wire_body_read_error(failure: &WireBodyReadError) -> NetworkControlError {
    match failure {
        WireBodyReadError::Timeout => NetworkControlError::Timeout,
        WireBodyReadError::RateLimited => NetworkControlError::RateLimited,
        WireBodyReadError::ResponseTooLarge => NetworkControlError::ResponseTooLarge,
        WireBodyReadError::Transport => {
            NetworkControlError::Transport("response body read failed".to_owned())
        }
    }
}
