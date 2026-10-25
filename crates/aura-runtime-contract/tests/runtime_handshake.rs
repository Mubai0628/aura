use aura_domain_common::{ContentHash, ContentHashAlgorithm};
use aura_runtime_contract::{runtime_bootstrap_proof, verify_runtime_bootstrap_proof};

#[test]
fn bootstrap_proof_matches_an_independent_hmac_sha256_vector() {
    let transcript_hash =
        ContentHash::new(ContentHashAlgorithm::Sha256, "11".repeat(32)).expect("SHA-256 fixture");
    let secret = [0x33_u8; 32];
    let proof = runtime_bootstrap_proof(&secret, &transcript_hash).expect("bootstrap proof");
    assert_eq!(proof.expose(), "3d65701d6095d32de7a4ef14732d32b8f1dbfc6850bd252f5f3a43d9bdad5a57");
    assert!(
        verify_runtime_bootstrap_proof(&secret, &transcript_hash, &proof)
            .expect("proof verification")
    );
}

#[test]
fn bootstrap_proof_rejects_secret_or_transcript_changes() {
    let transcript_hash =
        ContentHash::new(ContentHashAlgorithm::Sha256, "11".repeat(32)).expect("SHA-256 fixture");
    let changed_hash =
        ContentHash::new(ContentHashAlgorithm::Sha256, "12".repeat(32)).expect("SHA-256 fixture");
    let secret = [0x33_u8; 32];
    let proof = runtime_bootstrap_proof(&secret, &transcript_hash).expect("bootstrap proof");
    assert!(
        !verify_runtime_bootstrap_proof(&[0x34_u8; 32], &transcript_hash, &proof)
            .expect("changed secret verification")
    );
    assert!(
        !verify_runtime_bootstrap_proof(&secret, &changed_hash, &proof)
            .expect("changed transcript verification")
    );
}
