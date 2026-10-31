use super::*;
/// Retain repository identity while removing authentication material from durable facts.
pub(super) fn normalize(
    value: &str,
    root: &Path,
) -> Result<(String, aura_domain_common::jcs::CanonicalSha256HashV1)> {
    ensure!(!value.chars().any(char::is_control), "git.invalid_remote_url");
    if let Some((helper, _)) = value.split_once("::")
        && !helper.is_empty()
        && helper.bytes().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'+' | b'-' | b'.'))
    {
        anyhow::bail!(
            "git.opaque_remote: 不透明远端助手无法绑定明确的仓库URL；请在终端处理或配置标准Git远端地址"
        );
    }
    if value.contains("://") {
        let mut url =
            url::Url::parse(value).map_err(|_| anyhow::anyhow!("git.invalid_remote_url"))?;
        ensure!(
            matches!(url.scheme(), "http" | "https" | "ssh" | "git+ssh" | "git" | "file")
                && (url.scheme() == "file" || url.host_str().is_some()),
            "git.opaque_remote: 不透明远端助手无法绑定明确的仓库URL；请在终端处理或配置标准Git远端地址"
        );
        let ssh_user = matches!(url.scheme(), "ssh" | "git+ssh").then(|| url.username().to_owned());
        let _ = url.set_username("");
        let _ = url.set_password(None);
        let fingerprint = aura_domain_common::jcs::canonical_sha256(&(url.as_str(), ssh_user))?;
        url.set_query(None);
        url.set_fragment(None);
        return Ok((url.to_string(), fingerprint));
    }
    // scp-style SSH (including a configured username) has no URL scheme.
    let separator = if let Some(bracket) = value.find('[') {
        value[bracket..].find("]:").map(|offset| bracket + offset + 1)
    } else {
        value.find(':')
    };
    if let Some((host, path)) = separator.map(|index| (&value[..index], &value[index + 1..]))
        && !host.contains(['/', '\\'])
        && host.len() > 1
    {
        let (user, host) =
            host.rsplit_once('@').map_or((None, host), |(user, host)| (Some(user), host));
        let host = host.to_ascii_lowercase();
        let url = if path.starts_with('/') {
            format!("ssh://{host}{path}")
        } else {
            format!("ssh://{host}/~/{path}")
        };
        // scp-relative paths are resolved in the login home, unlike /absolute URL
        // paths. Never let two such spellings borrow the same endpoint consent.
        return Ok((url, aura_domain_common::jcs::canonical_sha256(&("scp", user, host, path))?));
    }
    let path = Path::new(value);
    let absolute = if path.is_absolute() { path.to_owned() } else { root.join(path) };
    // Resolve spelling only. canonicalize/open could contact a UNC share before
    // network approval; the destination is not inspected until Native dispatch.
    let absolute =
        std::path::absolute(absolute).map_err(|_| anyhow::anyhow!("git.invalid_remote_url"))?;
    let url = url::Url::from_file_path(&absolute)
        .map_err(|_| anyhow::anyhow!("git.invalid_remote_url"))?
        .to_string();
    Ok((url.clone(), aura_domain_common::jcs::canonical_sha256(&(&url, None::<String>))?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_hides_credentials_but_distinct_hidden_endpoints_do_not_share_consent() {
        let first = normalize(
            "https://temporary-user:fixture-secret@EXAMPLE.com:443/repo.git?token=fixture-a#part",
            Path::new("."),
        )
        .expect("URL");
        let second = normalize(
            "https://another:fixture-other@example.com/repo.git?token=fixture-b#part",
            Path::new("."),
        )
        .expect("URL");
        assert_eq!(first.0, "https://example.com/repo.git");
        assert_eq!(first.0, second.0);
        assert_ne!(first.1, second.1);
        let same = normalize(
            "https://different:password@example.com/repo.git?token=fixture-a#part",
            Path::new("."),
        )
        .expect("URL");
        assert_eq!(first, same, "userinfo is authentication, not a repository destination");
    }
    #[test]
    fn scp_relative_and_absolute_ssh_paths_cannot_borrow_the_same_target() {
        let relative = normalize("fixture-user@HOST:repo.git", Path::new(".")).expect("scp");
        let absolute = normalize("ssh://fixture-user@host/repo.git", Path::new(".")).expect("URL");
        assert_eq!(relative.0, "ssh://host/~/repo.git");
        assert_ne!(relative.1, absolute.1);
        let other_user = normalize("another-user@host:repo.git", Path::new(".")).expect("scp");
        assert_eq!(relative.0, other_user.0);
        assert_ne!(
            relative.1, other_user.1,
            "SSH login home is part of destination identity, without exposing the username"
        );
    }
    #[test]
    fn a_local_destination_is_normalized_without_opening_it() {
        let root = tempfile::tempdir().expect("temporary spelling root");
        let normalized =
            normalize("never-created.git", root.path()).expect("does not access destination");
        assert!(normalized.0.ends_with("/never-created.git"));
        assert!(!root.path().join("never-created.git").exists());
    }
    #[test]
    fn opaque_helpers_are_rejected_without_disclosing_their_payload_and_ipv6_scp_remains_valid() {
        for remote in [
            "ext::command --password=temporary-secret",
            "helper::opaque-temporary-secret",
            "custom://opaque/address?token=temporary-secret",
        ] {
            let error = normalize(remote, Path::new("."))
                .expect_err("not an explicit repository URL")
                .to_string();
            assert!(error.contains("git.opaque_remote") && error.contains("终端"));
            assert!(!error.contains("temporary-secret"));
        }
        assert_eq!(
            normalize("fixture-user@[::1]:repo.git", Path::new(".")).expect("IPv6 scp").0,
            "ssh://[::1]/~/repo.git"
        );
    }
}
