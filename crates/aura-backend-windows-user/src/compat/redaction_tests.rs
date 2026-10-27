use super::redaction::{Cover, Redactor};

fn secrets(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

fn run(redactor: &mut Redactor, pieces: &[&str]) -> String {
    let mut out = Vec::new();
    for piece in pieces {
        out.extend(redactor.push(piece.as_bytes()));
    }
    out.extend(redactor.finish());
    String::from_utf8(out).expect("utf-8")
}

#[test]
fn a_value_split_across_pieces_is_still_hidden() {
    let mut redactor = Redactor::new(&secrets(&[("DB_PASSWORD", "hunter2-long")]), Cover::Label);
    let text = run(&mut redactor, &["user=root pass=hun", "ter2-", "long done"]);
    assert_eq!(text, "user=root pass=[已隐藏:DB_PASSWORD] done");
}

#[test]
fn several_values_and_a_value_inside_another_are_hidden_by_the_longest() {
    let mut redactor = Redactor::new(
        &secrets(&[("SHORT_TOKEN", "abcd"), ("LONG_TOKEN", "abcdyyyy"), ("API_KEY", "k3y-value")]),
        Cover::Label,
    );
    let text = run(&mut redactor, &["a=abcdyyyy b=abcd c=k3y-value"]);
    assert_eq!(text, "a=[已隐藏:LONG_TOKEN] b=[已隐藏:SHORT_TOKEN] c=[已隐藏:API_KEY]");
}

#[test]
fn a_utf8_value_and_its_url_encoded_form_are_hidden() {
    let mut redactor = Redactor::new(&secrets(&[("MY_SECRET", "密码 a/b")]), Cover::Label);
    let text = run(&mut redactor, &["raw 密码 a/b and %E5%AF%86%E7%A0%81%20a%2Fb", " end"]);
    assert_eq!(text, "raw [已隐藏:MY_SECRET] and [已隐藏:MY_SECRET] end");
}

#[test]
fn a_short_value_is_left_and_text_that_only_starts_like_a_value_is_released() {
    let mut redactor = Redactor::new(
        &secrets(&[("PIN_PASS", "123"), ("SERVICE_TOKEN", "tok-9876")]),
        Cover::Label,
    );
    assert_eq!(redactor.push(b"code 123 tok-98"), b"code 123 ".to_vec(), "the tail waits");
    assert_eq!(redactor.push(b"x"), b"tok-98x".to_vec(), "and is released once it differs");
    assert_eq!(redactor.push(b" tok-"), b" ".to_vec());
    assert_eq!(redactor.finish(), b"tok-".to_vec(), "the end releases what never completed");
}

#[test]
fn a_terminal_gets_one_star_per_character() {
    let mut redactor = Redactor::new(&secrets(&[("SANDBOX_TOKEN", "s3cr3t-tok")]), Cover::Stars);
    let text = run(&mut redactor, &["\u{1b}[32mtoken s3cr3t", "-tok\u{1b}[0m"]);
    assert_eq!(text, "\u{1b}[32mtoken **********\u{1b}[0m");
}

#[test]
fn nothing_to_hide_passes_output_through() {
    let mut redactor = Redactor::new(&[], Cover::Label);
    assert!(redactor.is_empty());
    assert_eq!(redactor.push(b"plain"), b"plain".to_vec());
}
