use super::*;

fn assert_within_reserve(text: String, kind: BudgetTextKind, observed: u64) {
    let budget = ContextEstimatorRegistry::default()
        .estimate_budget(
            &ModelEstimatorKey {
                provider_id: ProviderId::new(),
                provider_model_name: "generic-model".to_owned(),
            },
            128_000,
            4_096,
            &[CanonicalTextBudgetItem {
                ordinal: 1,
                content_hash: ContentHash::sha256(text.as_bytes()),
                canonical_text: text,
                kind,
            }],
            &[],
        )
        .expect("budget without prior input calibration");
    assert!(
        budget.estimated_input_tokens.abs_diff(observed) <= budget.safety_reserve_tokens,
        "estimated {}, observed {observed}, safety reserve {}",
        budget.estimated_input_tokens,
        budget.safety_reserve_tokens,
    );
}

fn encoded_record(length: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut seed = 947_521_u32;
    (0..length)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            char::from(ALPHABET[(seed >> 26) as usize])
        })
        .collect()
}

#[test]
fn estimates_first_request_without_prior_input_usage() {
    let prompt = "Choose the best answer and show how you arrived at it.\n".repeat(2_000);
    assert_within_reserve(prompt, BudgetTextKind::Text, 24_000);
}

#[test]
fn reference_count_fits_a_small_worker_stack() {
    let count = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let json = "{\"state\": \"东京 and Kraków\", \"seq\": 12996}\n".repeat(1_200);
            reference_text_tokens(&json)
        })
        .expect("start tokenizer thread")
        .join()
        .expect("finish tokenizer count");
    assert!(count > 0);
}

#[test]
fn estimates_native_reasoning_without_counting_ciphertext_as_prompt_text() {
    let ciphertext = encoded_record(120_000);
    let kind = estimate_native_reasoning(None, &[], ciphertext.len() as u64);
    assert_within_reserve(ciphertext, kind, 12_000);
}

#[test]
fn estimates_ciphertext_embedded_as_literal_summary_source() {
    let prompt =
        format!("Summarize the following sources.\n{{\"opaque\":\"{}\"}}", encoded_record(50_000));
    assert_within_reserve(prompt, BudgetTextKind::Serialized, 34_000);
}
