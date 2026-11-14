//! Core's own sentence for an HTTP tool's transfer that did not finish within the gateway's time
//! limit: what timed out, how much arrived, what was not written, and what to try next.
use crate::controlled_web::proxy::{explain_transport, is_timeout, target_of, worded};
use aura_network_control::NetworkControlError;

pub(super) fn download_timeout(url: &url::Url, timeout_ms: u64, received: u64) -> String {
    let target = target_of(url).unwrap_or_else(|| "目标".to_owned());
    let seconds = timeout_ms.div_ceil(1000);
    let arrived = arrived(received);
    format!(
        "network.timeout: 下载超时：{target} 在 {seconds} 秒内没有传完（{arrived}），文件没有写入。可以稍后重试或换一个下载地址；网站慢时，可以在 设置 → 模型 → 网络代理 里用代理。"
    )
}

fn arrived(received: u64) -> String {
    if received == 0 {
        "没有收到数据".to_owned()
    } else {
        format!("已收到 {}", size(received))
    }
}

/// A failure after the server began to answer: the request was sent, so the line says how much
/// arrived and that nothing was written, never the transport's own words.
fn body_failure(error: &NetworkControlError, received: u64) -> String {
    let code = aura_network_control::ordinary_tools::safe_transport_code(error);
    let arrived = arrived(received);
    match innermost(error) {
        NetworkControlError::ResponseTooLarge | NetworkControlError::DownloadLimitExceeded => {
            format!(
                "{code}: 文件超过了下载大小的上限 {}，已停止接收（{arrived}），文件没有写入。可以换一个更小的文件，或请用户在浏览器里下载。",
                size(aura_web_domain::HTTP_DOWNLOAD_LIMIT)
            )
        }
        NetworkControlError::ContentEncodingUnsupported
        | NetworkControlError::DecompressionFailed => format!(
            "{code}: 下载的内容没能解压（{arrived}），文件没有写入。可以重新下载，或换一个下载地址。"
        ),
        _ => format!(
            "{code}: 下载到一半连接断了（{arrived}），文件没有写入。可以重新下载；网站慢或不稳定时，可以在 设置 → 模型 → 网络代理 里用代理。"
        ),
    }
}

fn innermost(error: &NetworkControlError) -> &NetworkControlError {
    match error {
        NetworkControlError::RoutedTransport { error, .. } => innermost(error),
        other => other,
    }
}

/// A download's failure in the model's words: its own sentence when the time ran out, else the
/// gateway's usual text.
pub(super) fn explain(
    error: NetworkControlError,
    url: Option<&url::Url>,
    timeout_ms: u64,
    received: u64,
) -> anyhow::Error {
    match url {
        Some(url) if is_timeout(&error) => {
            worded(&error, download_timeout(url, timeout_ms, received))
        }
        _ => explain_transport(error),
    }
}

/// The same for a failure while the body arrived.
pub(super) fn explain_body(
    error: &NetworkControlError,
    url: Option<&url::Url>,
    timeout_ms: u64,
    received: u64,
) -> anyhow::Error {
    match url {
        Some(url) if is_timeout(error) => {
            worded(error, download_timeout(url, timeout_ms, received))
        }
        _ => anyhow::anyhow!(body_failure(error, received)),
    }
}

/// Bytes in binary units with one decimal.
fn size(bytes: u64) -> String {
    for (unit, scale) in [("GB", 1u64 << 30), ("MB", 1 << 20), ("KB", 1 << 10)] {
        if bytes >= scale {
            let tenths = (u128::from(bytes) * 10 + u128::from(scale) / 2) / u128::from(scale);
            return format!("{}.{} {unit}", tenths / 10, tenths % 10);
        }
    }
    format!("{bytes} 字节")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controlled_web::proxy::is_timeout;
    use aura_network_control::NetworkControlError;

    #[test]
    fn a_download_timeout_says_what_arrived_and_what_to_try() {
        let url = url::Url::parse("https://github.com/owner/repo/releases/x.zip").expect("url");
        let line = download_timeout(&url, 300_000, 43_200_000);
        assert!(line.starts_with("network.timeout: 下载超时"), "{line}");
        for words in [
            "github.com:443",
            "300 秒",
            "41.2 MB",
            "文件没有写入",
            "重试",
            "设置 → 模型 → 网络代理",
        ] {
            assert!(line.contains(words), "{words} in {line}");
        }
        assert!(!line.contains("/owner/repo"), "no path: {line}");
        let none = download_timeout(&url, 300_000, 0);
        assert!(none.contains("没有收到数据"), "{none}");
        assert!(is_timeout(&NetworkControlError::Timeout));
        assert!(is_timeout(&NetworkControlError::RoutedTransport {
            error: Box::new(NetworkControlError::Timeout),
            routing: vec![],
            request_not_started: false,
        }));
        assert!(!is_timeout(&NetworkControlError::ResponseTooLarge));
    }

    /// A body that stopped arriving keeps its code and says how much arrived, that nothing was
    /// written, and what to try; the transport's own words stay out.
    #[test]
    fn a_body_failure_says_what_arrived_and_never_the_transport_s_words() {
        let large =
            explain_body(&NetworkControlError::DownloadLimitExceeded, None, 0, 3 << 20).to_string();
        assert!(large.starts_with("network.response_too_large: 文件超过了下载大小的上限"));
        assert!(large.contains("256.0 MB") && large.contains("已收到 3.0 MB"), "{large}");
        let reset = explain_body(
            &NetworkControlError::RoutedTransport {
                error: Box::new(NetworkControlError::Transport("os error 10054 raw".into())),
                routing: vec![],
                request_not_started: false,
            },
            None,
            0,
            0,
        )
        .to_string();
        assert!(reset.starts_with("network.") && reset.contains("连接断了"), "{reset}");
        assert!(reset.contains("没有收到数据") && !reset.contains("10054"), "{reset}");
        assert!(reset.len() <= 512 && !reset.contains('\n'), "{reset}");
    }
}
