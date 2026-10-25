pub(super) fn worker_confirmed(error: Option<&anyhow::Error>) -> bool {
    !error.is_some_and(|error| {
        error.chain().any(|cause| {
            matches!(
                cause.downcast_ref::<crate::agent_store::StoreError>(),
                Some(crate::agent_store::StoreError::Unknown { .. })
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s07_n11_an_unknown_filesystem_worker_is_never_reported_as_confirmed_stopped() {
        let error = anyhow::Error::new(crate::agent_store::StoreError::Unknown {
            operation: "agent.http.download_publish",
            waited_ms: 10,
            receipt: Some(1),
        })
        .context("download publication wait");
        assert!(!worker_confirmed(Some(&error)));
    }

    #[test]
    fn s07_n11_a_reaped_download_or_normal_refusal_has_confirmed_stopped() {
        assert!(worker_confirmed(None));
        assert!(worker_confirmed(Some(&anyhow::anyhow!("http.cancelled"))));
    }
}
