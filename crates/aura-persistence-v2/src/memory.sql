-- File-derived caches. One independent Vault per cache; never a project catalogue.
CREATE TABLE memory_vaults (
    vault_id TEXT PRIMARY KEY NOT NULL,
    singleton INTEGER NOT NULL UNIQUE CHECK(singleton=1),
    scope TEXT NOT NULL CHECK(scope IN ('global','project')),
    workspace_id TEXT,
    root_identity_json TEXT,
    state TEXT NOT NULL,
    CHECK((scope='global' AND workspace_id IS NULL) OR (scope='project' AND workspace_id IS NOT NULL))
) STRICT;
CREATE TABLE memory_entries (
    relative_path TEXT PRIMARY KEY NOT NULL,
    memory_id TEXT,
    title TEXT NOT NULL,
    category TEXT,
    status TEXT,
    importance INTEGER,
    confidence REAL,
    recall_weight REAL,
    updated_at TEXT,
    content_hash TEXT NOT NULL,
    valid INTEGER NOT NULL CHECK(valid IN (0,1)),
    invalid_reason TEXT,
    metadata_json TEXT,
    indexed_at TEXT NOT NULL
) STRICT;
CREATE INDEX memory_entries_id ON memory_entries(memory_id);
CREATE VIRTUAL TABLE memory_fts USING fts5(relative_path UNINDEXED, title, body, tokenize='trigram');
CREATE TABLE memory_links (
    source_path TEXT NOT NULL REFERENCES memory_entries(relative_path) ON DELETE CASCADE,
    target_id TEXT NOT NULL,
    relation TEXT NOT NULL CHECK(relation IN ('related','supersedes','conflicts','wikilink')),
    PRIMARY KEY(source_path,target_id,relation)
) STRICT;
CREATE TABLE memory_history (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    record_hash TEXT NOT NULL UNIQUE,
    memory_id TEXT,
    relative_path TEXT NOT NULL,
    action TEXT NOT NULL,
    actor TEXT NOT NULL,
    session_id TEXT,
    turn_id TEXT,
    before_hash TEXT,
    after_hash TEXT,
    title TEXT NOT NULL,
    at TEXT NOT NULL
) STRICT;
CREATE TABLE memory_maintenance (
    run_id TEXT PRIMARY KEY NOT NULL,
    runtime_instance_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_id TEXT,
    state TEXT NOT NULL,
    reason TEXT,
    model_profile_id TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    record_json TEXT NOT NULL
) STRICT;
