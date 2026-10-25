//! The frozen profile of the Aura Production Release Root certificate. Shared by the build script,
//! which refuses a production build with a root outside it, and by the release key tests, which
//! check that the release key tooling issues exactly this profile.
use x509_parser::{prelude::*, public_key::PublicKey};

pub fn validate_production_root(der: &[u8]) -> Result<(), String> {
    let (remaining, certificate) =
        X509Certificate::from_der(der).map_err(|error| format!("DER parse failed: {error}"))?;
    require(remaining.is_empty(), "DER contains trailing bytes")?;
    require(certificate.version() == X509Version::V3, "certificate is not X.509 v3")?;
    require(certificate.subject() == certificate.issuer(), "certificate is not self-issued")?;
    certificate
        .verify_signature(None)
        .map_err(|error| format!("self-signature verification failed: {error}"))?;

    let common_names = certificate
        .subject()
        .iter_common_name()
        .map(|value| value.as_str().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let organizations = certificate
        .subject()
        .iter_organization()
        .map(|value| value.as_str().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    require(
        common_names == ["Aura Production Release Root v1"],
        "Subject Common Name is not the frozen Production Root name",
    )?;
    require(organizations == ["Aura Project"], "Subject Organization is not Aura Project")?;
    require(
        certificate.subject().iter_attributes().count() == 2,
        "Subject contains attributes outside the frozen CN/O profile",
    )?;

    let basic = certificate
        .basic_constraints()
        .map_err(|error| format!("Basic Constraints are invalid: {error}"))?
        .ok_or_else(|| "Basic Constraints are missing".to_owned())?;
    require(basic.critical, "Basic Constraints are not critical")?;
    require(
        basic.value.ca && basic.value.path_len_constraint == Some(1),
        "Basic Constraints must be CA=true,pathLen=1",
    )?;

    let usage = certificate
        .key_usage()
        .map_err(|error| format!("Key Usage is invalid: {error}"))?
        .ok_or_else(|| "Key Usage is missing".to_owned())?;
    require(usage.critical, "Key Usage is not critical")?;
    require(
        usage.value.key_cert_sign()
            && usage.value.crl_sign()
            && !usage.value.digital_signature()
            && !usage.value.non_repudiation()
            && !usage.value.key_encipherment()
            && !usage.value.data_encipherment()
            && !usage.value.key_agreement()
            && !usage.value.encipher_only()
            && !usage.value.decipher_only(),
        "Key Usage must contain only keyCertSign and cRLSign",
    )?;
    require(
        certificate
            .extended_key_usage()
            .map_err(|error| format!("Extended Key Usage is invalid: {error}"))?
            .is_none(),
        "Production Root must not contain Extended Key Usage",
    )?;

    certificate
        .extensions_map()
        .map_err(|error| format!("certificate has duplicate extensions: {error}"))?;
    let has_ski = certificate
        .extensions()
        .iter()
        .any(|extension| extension.oid.to_id_string() == "2.5.29.14");
    let has_aki = certificate
        .extensions()
        .iter()
        .any(|extension| extension.oid.to_id_string() == "2.5.29.35");
    require(
        has_ski && has_aki,
        "Subject Key Identifier and Authority Key Identifier are required",
    )?;

    require(
        certificate.signature_algorithm.algorithm.to_id_string() == "1.2.840.10045.4.3.2"
            && certificate.tbs_certificate.signature.algorithm.to_id_string()
                == "1.2.840.10045.4.3.2",
        "certificate signature algorithm must be ECDSA-with-SHA256",
    )?;
    let public_key = certificate.public_key();
    require(
        public_key.algorithm.algorithm.to_id_string() == "1.2.840.10045.2.1",
        "Production Root public key algorithm must be id-ecPublicKey",
    )?;
    let curve = public_key
        .algorithm
        .parameters
        .as_ref()
        .and_then(|parameters| parameters.as_oid().ok())
        .map(|oid| oid.to_id_string());
    require(
        curve.as_deref() == Some("1.2.840.10045.3.1.7"),
        "Production Root public key curve must be P-256",
    )?;
    require(
        matches!(public_key.parsed(), Ok(PublicKey::EC(key)) if key.key_size() == 256),
        "Production Root public key is not a 256-bit EC point",
    )?;

    let serial = certificate.raw_serial();
    let significant_serial = serial.strip_prefix(&[0]).unwrap_or(serial);
    require(
        significant_serial.len() >= 16 && significant_serial.iter().any(|byte| *byte != 0),
        "Production Root serial must be a positive random value of at least 128 bits",
    )?;
    require(certificate.validity().is_valid(), "Production Root is not currently valid")?;
    Ok(())
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    condition.then_some(()).ok_or_else(|| message.to_owned())
}
