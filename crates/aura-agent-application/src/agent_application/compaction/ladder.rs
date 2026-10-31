//! What a compaction asks after a failed attempt. The step's own request, which the provider has
//! cached, is kept for as long as it can still give the summary: sent again when no answer came,
//! and with the answer's tool calls answered when it called tools. Only then is the isolated
//! request asked, which shares nothing with it that could fail it the same way. An answer cut off
//! at its limit goes that way too: the step's request already had all the output the model's
//! limit and the window allow.
use aura_context_runtime::{FailedCompactionAttempt, UnusableAnswer};

/// How often one request is sent again after failures that left no answer.
const RESENDS: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Rung {
    /// The failed request again.
    Resend,
    /// The failed request with its answer's tool calls answered: no tool runs.
    AnswerTools,
    /// The isolated request.
    Isolated,
}

/// The attempt that failed, as the next one is chosen for it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Failure {
    pub how: FailedCompactionAttempt,
    /// It was asked on the step's request.
    pub on_prefix: bool,
}

/// What has been tried after the first attempt.
#[derive(Debug, Default)]
pub(super) struct Ladder {
    resent: u8,
    answered_tools: bool,
    isolated: bool,
    asked_again: bool,
}

impl Ladder {
    /// The next attempt, or none. `isolated` when the isolated request fits its window; `manual`
    /// when the user asked for the compaction.
    pub(super) fn next(&mut self, failed: Failure, isolated: bool, manual: bool) -> Option<Rung> {
        use FailedCompactionAttempt::{NoAnswer, UnusableAnswer as Unusable};
        let next = match failed.how {
            NoAnswer { retryable: true } if self.resent < RESENDS => {
                self.resent += 1;
                Rung::Resend
            }
            Unusable { why: UnusableAnswer::ToolCalls }
                if failed.on_prefix && !self.answered_tools =>
            {
                self.answered_tools = true;
                Rung::AnswerTools
            }
            _ if failed.on_prefix && isolated => {
                self.isolated = true;
                Rung::Isolated
            }
            // Asked the isolated way from the start, or one the user asked for that has no other
            // request to send: the same request once more. The isolated request after the step's
            // is the last; Core's own compaction on the step's request takes the history a part
            // at a time instead, which the caller decides.
            Unusable { .. }
                if (!failed.on_prefix && !self.isolated || failed.on_prefix && manual)
                    && !self.asked_again =>
            {
                self.asked_again = true;
                Rung::Resend
            }
            _ => return None,
        };
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_prefix(how: FailedCompactionAttempt) -> Failure {
        Failure { how, on_prefix: true }
    }

    fn isolated(how: FailedCompactionAttempt) -> Failure {
        Failure { how, on_prefix: false }
    }

    fn unusable(why: UnusableAnswer) -> FailedCompactionAttempt {
        FailedCompactionAttempt::UnusableAnswer { why }
    }

    const LOST: FailedCompactionAttempt = FailedCompactionAttempt::NoAnswer { retryable: true };

    #[test]
    fn a_request_that_got_no_answer_is_sent_twice_more_then_the_isolated_way() {
        let mut ladder = Ladder::default();
        let steps = (0..3).map(|_| ladder.next(on_prefix(LOST), true, false));
        assert_eq!(
            steps.collect::<Vec<_>>(),
            [Some(Rung::Resend), Some(Rung::Resend), Some(Rung::Isolated)]
        );
        let mut ladder = Ladder::default();
        ladder.next(on_prefix(LOST), false, false);
        ladder.next(on_prefix(LOST), false, false);
        assert_eq!(ladder.next(on_prefix(LOST), false, false), None, "no isolated room");
        let refused = FailedCompactionAttempt::NoAnswer { retryable: false };
        assert_eq!(Ladder::default().next(on_prefix(refused), true, false), Some(Rung::Isolated));
    }

    #[test]
    fn an_answer_that_called_tools_has_them_answered_once_on_the_same_request() {
        let mut ladder = Ladder::default();
        let called = on_prefix(unusable(UnusableAnswer::ToolCalls));
        assert_eq!(ladder.next(called, true, false), Some(Rung::AnswerTools));
        assert_eq!(ladder.next(called, true, false), Some(Rung::Isolated));
        assert_eq!(ladder.next(isolated(unusable(UnusableAnswer::Other)), true, false), None);
        let mut ladder = Ladder::default();
        ladder.next(called, false, false);
        assert_eq!(ladder.next(called, false, false), None, "the caller takes parts");
    }

    #[test]
    fn another_unusable_answer_goes_the_isolated_way_or_is_asked_again_once_where_none_follows() {
        let other = unusable(UnusableAnswer::Other);
        assert_eq!(Ladder::default().next(on_prefix(other), true, false), Some(Rung::Isolated));
        assert_eq!(Ladder::default().next(on_prefix(other), false, false), None);
        let mut manual = Ladder::default();
        assert_eq!(manual.next(on_prefix(other), false, true), Some(Rung::Resend));
        assert_eq!(manual.next(on_prefix(other), false, true), None);
        let mut ladder = Ladder::default();
        assert_eq!(ladder.next(isolated(other), true, false), Some(Rung::Resend));
        assert_eq!(ladder.next(isolated(other), true, false), None);
        for manual in [false, true] {
            let mut ladder = Ladder::default();
            assert_eq!(ladder.next(on_prefix(other), true, manual), Some(Rung::Isolated));
            assert_eq!(
                ladder.next(isolated(other), true, manual),
                None,
                "the last after the step's"
            );
        }
        let called = isolated(unusable(UnusableAnswer::ToolCalls));
        assert_eq!(
            Ladder::default().next(called, true, false),
            Some(Rung::Resend),
            "no tools there"
        );
    }

    /// The longest way any compaction can take stays within the attempts one may make.
    #[test]
    fn no_way_through_the_ladder_passes_the_attempt_limit() {
        let called = unusable(UnusableAnswer::ToolCalls);
        let other = unusable(UnusableAnswer::Other);
        let ways: [(&[Failure], bool, bool); 3] = [
            (
                &[on_prefix(LOST), on_prefix(LOST), on_prefix(called), on_prefix(called)],
                true,
                false,
            ),
            (&[on_prefix(LOST), on_prefix(LOST), on_prefix(called), on_prefix(other)], false, true),
            (&[isolated(other), isolated(LOST), isolated(LOST)], true, false),
        ];
        for (failures, fits, manual) in ways {
            let mut ladder = Ladder::default();
            let asked = failures.iter().take_while(|f| ladder.next(**f, fits, manual).is_some());
            let attempts = 1 + asked.count();
            assert_eq!(attempts, failures.len() + 1, "every failure asked again: {failures:?}");
            assert!(attempts <= usize::from(aura_context_domain::MAX_COMPACTION_ATTEMPTS));
        }
    }
}
