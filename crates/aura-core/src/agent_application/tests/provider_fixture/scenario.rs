use super::*;

/// The workspace file the compaction-after-tool scenario reads; the test writes it larger than
/// the retention budget so the reading call is the part compaction replaces.
pub const COMPACTION_SOURCE_FILE: &str = "compaction-source.txt";
/// The file a compaction answered with a tool call asks to write; it must never appear.
pub const COMPACTION_TOOL_FILE: &str = "written-by-a-compaction.txt";

#[derive(Clone, Copy)]
pub enum Scenario {
    /// All three real wire formats: query, tool continuation, output-limit Core continuation.
    Activity {
        format: ModelApiFormat,
        retry: bool,
    },
    Write,
    /// `Write` with a 40,000-character text, longer than a repeated argument may be.
    LongWrite,
    /// One `web.search`, answered by this server as DeepSeek's search, then an answer.
    DeepSeekWebSearch,
    /// One `web.fetch` of a page DeepSeek's fetch answers 404 for, then an answer.
    DeepSeekWebFetchMissing,
    /// `tools.call` runs `agent.activity.query`, which is not in the request's list; then
    /// `tools.load` adds it; then an answer. Every later request is answered in plain text.
    UnifiedEntry,
    /// `tools.call` names `goal.complete` in a Normal turn, which the entry never runs;
    /// then an answer.
    EntryRefused,
    /// `tools.call` writes the external file the user message names as
    /// `AURA_EXTERNAL_PATH=<absolute path>`.
    EntryExternalWrite,
    /// `tools.call` runs `agent.activity.query`, then an answer.
    EntryHookQuery,
    /// One browser call (the tool and its arguments as JSON text), then an answer.
    Browser {
        tool: &'static str,
        arguments: &'static str,
    },
    /// Several calls in one turn, each after the previous one's result, then an
    /// answer; each is a tool and its arguments as JSON text.
    Sequence {
        calls: &'static [(&'static str, &'static str)],
    },
    /// `goal.complete` citing the files it wrote
    /// as its evidence, then - if that was refused - `normal.complete`, as DeepSeek did.
    GoalWithPaths,
    /// `goal.complete` citing an artifact id that is not this run's, then `normal.complete`.
    GoalWithUnknownArtifact,
    /// The `Write` turn (find, load and run one write, then answer), then a plain answer
    /// to every later request, from any conversation: a branch and its source both continue.
    Branching,
    /// The `CompactAfterTool` turn — a read of a large file that a compaction then
    /// replaces — then a plain answer to every later request, from any conversation.
    BranchingCompaction,
    /// The `Write` turn, then two oversized responses in the next request so the branch's
    /// own compaction has to take in the history it carried, then plain answers.
    BranchingThenCompact,
    Text,
    /// A plain answer whose Provider reports what it read from its cache (600 of 1000).
    CachedText,
    Query,
    Hold,
    Plan,
    Goal,
    Compact,
    CompactHold,
    /// Plain answers, and a compaction answered the first time it is asked.
    CompactOnPrefix,
    /// Plain answers; the first compaction is answered with a write that must never run.
    CompactToolOnPrefix,
    /// Eleven reads, of `read-0.txt` to `read-10.txt`, then an answer; the tenth request
    /// is parked until `release_window`, so a test can set the window from its size.
    ClearOldResults,
    /// A Tool call that reads a file larger than the retention budget, then one oversized
    /// response: compaction replaces the call, and the request after it must no longer carry
    /// the call's wire binding.
    CompactAfterTool,
    Search,
    /// Discover and run one real search, then answer in plain text. The first
    /// request stays parked until `release_window` is notified, so a test can
    /// accept an addition after that request is frozen.
    SearchThenText,
    /// Discover and read one file the test names in the user message as
    /// `AURA_EXTERNAL_PATH=<absolute path>`, then answer. Used to drive the
    /// authorized-external-target path through the real model loop.
    ExternalRead,
    /// The same call against a path no grant authorizes: the refusal must be the
    /// corrigible outside-workspace class and the file must stay unread.
    ExternalReadRefused,
    /// The same call, with no expectation about the outcome: the test asserts what a
    /// given mode really decided. Used for the permission matrix, where either an
    /// authorized read or a corrigible refusal is a valid answer but silently
    /// reading an unauthorized file is not.
    ExternalReadMatrix,
    /// List one authorized external directory named the same way, then answer.
    ExternalList,
    /// The same listing call with no expectation about its outcome: the test asserts what
    /// a given mode or grant scope really decided. Used for the depth boundary, where a
    /// refusal is the correct answer and a walk that reads through it is not.
    ExternalListOutcome,
    /// Find one file name under an authorized external directory, then answer.
    ExternalFind,
    /// The same recursive request against a directory whose grant is **not**
    /// recursive: the walk must stay inside the authorized depth, so a name that only
    /// exists in a subdirectory is not found and the answer is an honest empty page
    /// rather than a wider search.
    ExternalFindScopeCapped,
    /// Ask for a text search rooted at an external path. Text search is
    /// workspace-index based, so the test pins that it refuses rather than
    /// silently searching somewhere else.
    ExternalSearch,
    /// Create a file at an authorized external path named the same way, then
    /// answer. The path does not exist yet, so the call exercises the create
    /// target: a verified parent directory plus the name to publish.
    ExternalWrite,

    /// Park the first plain-text request the same way, then answer again after
    /// the addition has been consumed.
    AdditionText,
    /// Park, accept one oversized addition plus a short tail, compact the
    /// oversized addition, then finish the continuation in plain text.
    CompactedAddition,
    Window {
        failures: usize,
    },
    WindowHold,
    /// First step returns a call whose argument fragment is not valid JSON, the
    /// way a streaming Provider can emit an unescaped newline inside a string.
    MalformedArguments,
    /// First step ends the turn with no user-visible text at all.
    EmptyText,
    /// First step ends the turn with Reasoning only: no usable answer, so the
    /// protocol reminder protection must still apply.
    ThinkingOnly,
    /// First step ends at the output limit, so the answer is not trustworthy.
    TruncatedText,
    /// First step reasons until the output limit and is cut off before any answer or
    /// Tool call: the reminder has to say why the response was empty.
    ThinkingCutOff,
    /// Leave one real request interrupted, then complete later tasks without
    /// reactivating the interrupted lineage.
    RecoveredSession,
    /// Every model step closes the current turn. Mode completion stays separate.
    FinishTurn,
    /// Two complete EndTurns with no tools; C1 reminds once then stops.
    ProtocolLoop,
    /// Repeated expire_prior against the current lineage; C2 stops after two
    /// post-feedback failures.
    InvalidReconcile,
    /// Raw OpenAI Responses SSE: the first model step completes one real file
    /// write, the second names its function call `_aura`, which the frozen
    /// request never exposed. Later steps answer in plain text so a new human
    /// continuation can finish without dispatching anything.
    ResponsesWriteThenIllegalToolName,
    /// Unknown first, then one known write: the same turn must continue from the refusal.
    ResponsesUnknownThenWrite,
    /// A **definite** Provider failure: the real endpoint answers with an HTTP
    /// error status and an error document, so the step fails for a Provider
    /// reason and no tool is ever admitted.
    ProviderUnavailable,
    /// A real broken connection: the endpoint writes the response head and a
    /// partial SSE frame, then closes the socket in the middle of the response.
    /// The transport sees a truncated stream, not an error document.
    BrokenConnection,
    /// The provider starts streaming and then reports itself overloaded inside the
    /// stream, as the owner's gateway did, for the first `failures` requests; then plain answers.
    OverloadedThenText {
        failures: usize,
    },
    /// A real late answer: every request is parked until the test releases the
    /// window, which it does only **after** the caller's own budget has already
    /// ended the step. The answer is written into a socket nothing is reading.
    LateAnswer,
    /// Discover and read a workspace path that does not exist: the call really
    /// runs and the tool answers with a definite failure, which is neither a
    /// refusal before dispatch nor an unknown outcome.
    ReadMissing,
    /// `/init`: submit the reviewed AGENTS.md citing [`INIT_EVIDENCE_FILE`], then answer.
    Init,
    /// Read [`HOOK_SOURCE`] with the always-offered `fs.read`, then answer; the
    /// test's Hooks decide what is really read and what the model sees.
    HookRead,
    /// A plain answer to every request and a valid summary to every compaction request,
    /// so a test can shape the context with the window alone.
    Plain,
    /// `Plain`, with the Provider counting each request at a quarter of its bytes — about
    /// what a real tokenizer makes of prose and code — instead of a fixed count.
    PlainMeasured,
    /// `PlainMeasured`, except that the second request is refused outright, so that
    /// step ends with no count.
    PlainMeasuredRefusingSecond,
}

/// The file the `HookRead` scenario asks to read.
pub const HOOK_SOURCE: &str = "hook-source.txt";

/// The workspace file the `/init` scenario cites as the definition of its one command.
pub const INIT_EVIDENCE_FILE: &str = "README.md";
pub const INIT_COMMAND: &str = "cargo test --workspace";
