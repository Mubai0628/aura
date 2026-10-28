//! The conversation's own history, by round, for a model whose context holds only a summary of it.
use super::definitions::entry;
use super::*;
use AgentBuiltinHandler as H;
use aura_context_domain::*;
use aura_security_contract::ResolvedCapability as C;

const ROUND: &str = "A round (T1, T2…) is one task: from the user's request until it was completed; questions and answers, messages the user added and continuing after a stop or an error stay in the same round.";

pub(super) fn entries(
    at: UtcTimestamp,
) -> Result<Vec<AgentBuiltinEntry>, BuiltinToolDefinitionSetError> {
    let read = &[C::AgentTaskRead];
    [
        entry::<HistoryListTurnsInput, HistoryListTurnsResult>(
            104,
            H::HistoryListTurns,
            "List the conversation's rounds",
            &format!("List rounds of this conversation, each with its id, when its first message was sent and that message's first 200 characters (with its whole length): the last n (last), the first n (first), by id (ids), or begun in a time range (since, until); the last 10 by default. {ROUND} Read a whole first message with history.read (T5.1)."),
            read,
            false,
            at,
        ),
        entry::<HistorySearchInput, HistorySearchResult>(
            105,
            H::HistorySearch,
            "Search the conversation's history",
            &format!("Find entries of this conversation that contain every word of query, ignoring case: user messages, the model's thinking and replies, and tool calls with their arguments and results. Hits come newest first with their entry id (T3.17), kind, time and about 30 characters on each side of the first word; read an entry whole with history.read. turns limits the search to some rounds, kinds to some kinds of entry. complete is false when the search stopped early; searched_back_to says how far back it got. {ROUND}"),
            read,
            false,
            at,
        ),
        entry::<HistoryReadInput, HistoryReadResult>(
            106,
            H::HistoryRead,
            "Read entries of the conversation's history",
            "Read entries of this conversation whole by id (T3.17, from history.search; T5.1 is round 5's first message), up to 10 at once: a user message, the model's thinking or reply, or a tool call with its arguments and the result it gave. Each entry's text is paged: limit characters (default 8000, at most 64000) from offset; next_offset continues it.",
            read,
            false,
            at,
        ),
    ]
    .into_iter()
    .collect()
}
