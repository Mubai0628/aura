//! Core's lines for a download that did not finish: what the server answered or what happened to
//! the file, and what to do next. Each is one line of at most 512 bytes.

/// The code of a download whose file was put back as it was; the result records the rollback.
pub(in crate::controlled_web) const ROLLED_BACK: &str = "http.publication_rolled_back";

/// The session's permission changed while the file arrived.
pub(super) const PERMISSION_WITHDRAWN: &str = "agent.permission_withdrawn: 文件下载完了，但这次会话的权限在下载期间变了，不再允许写入，文件没有保存。可以重新调用，会按新的权限检查。";

/// A status that is not a success: the number, what it means, and what to try.
pub(super) fn status(status: u16) -> String {
    let (meaning, next) = match status {
        401 | 407 => ("下载需要登录或认证", "可以换一个公开的下载地址，或请用户在浏览器里下载。"),
        403 => ("服务器拒绝提供这个文件", "可以换一个下载地址，或请用户在浏览器里下载。"),
        404 | 410 => ("这个地址上没有文件", "请检查地址，或找到文件现在的下载地址。"),
        408 | 429 => ("服务器在限制请求或等得太久", "请稍等片刻再下载。"),
        300..=399 => ("服务器要求跳转，但没有给出可以跟随的地址", "请直接用最终的下载地址。"),
        500..=599 => ("服务器出错了", "可以稍后重试，或换一个下载地址。"),
        _ => ("这不是成功的回应", "请检查地址后再下载。"),
    };
    format!("http.download_status: 服务器回应 HTTP {status}，{meaning}，文件没有下载。{next}")
}

pub(super) fn poisoned() -> anyhow::Error {
    anyhow::anyhow!(
        "http.staging_poisoned: 下载用的临时文件在另一个步骤出错后不能再用，文件没有保存。可以重新下载。"
    )
}

pub(super) fn partial(status: u16) -> String {
    format!(
        "http.partial_response: 服务器回应 HTTP {status}，只提供了分段或增量内容，文件没有发布。可以换一个能返回完整文件的地址。"
    )
}

pub(super) fn finished() -> anyhow::Error {
    anyhow::anyhow!(
        "http.staging_finished: 下载用的临时文件已经收走（这次下载已经结束或被停止），文件没有保存。可以重新下载。"
    )
}

pub(super) fn never_published(path: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "http.never_published: 文件下载完了，但没能放到 {}，那里保持原样，文件没有保存。可以重新下载。",
        shown(path)
    )
}

pub(super) fn rolled_back(path: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{ROLLED_BACK}: 文件下载完了，放到 {} 之后没能完成收尾，已经恢复原样，文件没有保存。可以重新下载。",
        shown(path)
    )
}

pub(super) fn unconfirmed(path: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "http.publication_unconfirmed: 文件下载完了，但 Aura 没能确认它有没有放到 {}。再下载之前，先看看那里的文件是不是这次下载的。",
        shown(path)
    )
}

/// The model's own path, cut to its last 120 bytes, without control characters or braces.
fn shown(path: &str) -> String {
    let text: String = path
        .chars()
        .map(|c| if c.is_control() || c == '{' || c == '}' { ' ' } else { c })
        .collect();
    if text.len() <= 120 {
        return text;
    }
    let mut start = text.len() - 119;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &text[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each line starts with its code, says the file was not saved or what to check, and stays
    /// one bounded line for a long path.
    #[test]
    fn a_download_line_says_what_happened_to_the_file() {
        let long = format!("D:/{}/a.zip", "很长的文件夹".repeat(40));
        let not_found = status(404);
        assert!(not_found.starts_with("http.download_status: 服务器回应 HTTP 404，"));
        assert!(not_found.contains("没有文件") && not_found.contains("检查地址"), "{not_found}");
        assert!(status(503).contains("服务器出错了") && status(401).contains("登录"));
        for (line, code) in [
            (not_found.clone(), "http.download_status"),
            (partial(206), "http.partial_response"),
            (PERMISSION_WITHDRAWN.to_owned(), "agent.permission_withdrawn"),
            (poisoned().to_string(), "http.staging_poisoned"),
            (finished().to_string(), "http.staging_finished"),
            (never_published(&long).to_string(), "http.never_published"),
            (rolled_back(&long).to_string(), ROLLED_BACK),
            (unconfirmed(&long).to_string(), "http.publication_unconfirmed"),
        ] {
            assert!(line.starts_with(&format!("{code}: ")), "{line}");
            assert!(line.len() <= 512 && !line.contains(['\n', '{', '}']), "{line}");
        }
        let kept = unconfirmed(&long).to_string();
        assert!(kept.contains("a.zip") && kept.contains("先看看"), "{kept}");
    }
}
