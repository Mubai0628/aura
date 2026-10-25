//! Metadata-only origin constraints; secret placement remains a separate policy.
use crate::ToolCredentialContractErrorV1;

pub fn canonical_credential_origin(value: &str) -> Result<String, ToolCredentialContractErrorV1> {
    let url = url::Url::parse(value).map_err(|_| ToolCredentialContractErrorV1::InvalidRecord)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(ToolCredentialContractErrorV1::InvalidRecord);
    }
    Ok(url.origin().ascii_serialization())
}

pub fn canonical_credential_origins(
    values: &[String],
) -> Result<Vec<String>, ToolCredentialContractErrorV1> {
    if values.len() > 32 {
        return Err(ToolCredentialContractErrorV1::InvalidRecord);
    }
    let mut origins = values
        .iter()
        .map(|value| canonical_credential_origin(value))
        .collect::<Result<Vec<_>, _>>()?;
    origins.sort();
    origins.dedup();
    Ok(origins)
}

pub(crate) fn validate_origins(values: &[String]) -> Result<(), ToolCredentialContractErrorV1> {
    if canonical_credential_origins(values)? != values {
        return Err(ToolCredentialContractErrorV1::InvalidRecord);
    }
    Ok(())
}
