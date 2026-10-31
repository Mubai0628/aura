//! The one-time code that pairs the extension: eight digits, valid for a few minutes and five
//! tries, compared in constant time.
use aura_chrome_domain::protocol::{PAIRING_CODE_DIGITS, PairRefusal, constant_time_eq};
use std::time::{Duration, Instant};

pub(super) const MAX_ATTEMPTS: u8 = 5;

pub(super) struct PendingCode {
    code: String,
    expires: Instant,
    attempts: u8,
}

impl PendingCode {
    pub(super) fn issue(now: Instant, valid_for: Duration) -> anyhow::Result<Self> {
        Ok(Self { code: random_digits()?, expires: now + valid_for, attempts: 0 })
    }

    pub(super) fn code(&self) -> &str {
        &self.code
    }

    /// Whether `typed` is the code. A wrong code costs one try; an expired or exhausted code never
    /// matches again.
    pub(super) fn check(&mut self, typed: &str, now: Instant) -> Result<(), PairRefusal> {
        if now >= self.expires {
            return Err(PairRefusal::CodeExpired);
        }
        if self.attempts >= MAX_ATTEMPTS {
            return Err(PairRefusal::TooManyAttempts);
        }
        if constant_time_eq(typed.as_bytes(), self.code.as_bytes()) {
            return Ok(());
        }
        self.attempts += 1;
        Err(if self.attempts >= MAX_ATTEMPTS {
            PairRefusal::TooManyAttempts
        } else {
            PairRefusal::CodeInvalid
        })
    }
}

/// Uniform digits: bytes from 250 up are thrown away rather than folded onto the low digits.
fn random_digits() -> anyhow::Result<String> {
    let mut code = String::with_capacity(PAIRING_CODE_DIGITS);
    while code.len() < PAIRING_CODE_DIGITS {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes)
            .map_err(|error| anyhow::anyhow!("chrome.pairing_randomness: {error}"))?;
        for byte in bytes.into_iter().filter(|byte| *byte < 250) {
            if code.len() < PAIRING_CODE_DIGITS {
                code.push(char::from(b'0' + byte % 10));
            }
        }
    }
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_eight_digits_and_spends_its_tries_and_its_time() {
        let now = Instant::now();
        let mut pending = PendingCode::issue(now, Duration::from_secs(300)).expect("code");
        assert!(pending.code().len() == 8 && pending.code().bytes().all(|b| b.is_ascii_digit()));
        let right = pending.code().to_owned();
        let wrong = if right == "00000000" { "11111111" } else { "00000000" };
        for _ in 0..4 {
            assert_eq!(pending.check(wrong, now), Err(PairRefusal::CodeInvalid));
        }
        assert_eq!(pending.check(&right, now), Ok(()), "the fifth try is still open");
        assert_eq!(pending.check(wrong, now), Err(PairRefusal::TooManyAttempts));
        assert_eq!(pending.check(&right, now), Err(PairRefusal::TooManyAttempts), "exhausted");
        let mut late = PendingCode::issue(now, Duration::from_secs(300)).expect("code");
        let code = late.code().to_owned();
        assert_eq!(
            late.check(&code, now + Duration::from_secs(300)),
            Err(PairRefusal::CodeExpired)
        );
    }
}
