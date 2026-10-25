CREATE TABLE context_snapshots(id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), registry_snapshot_id TEXT, payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE TABLE prompt_bundles(id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), content_hash TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE TABLE context_compactions(id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), state TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), payload TEXT NOT NULL CHECK(json_valid(payload))) STRICT;
CREATE TABLE history_atomic_groups(session_id TEXT NOT NULL REFERENCES sessions(id), ordinal INTEGER NOT NULL CHECK(ordinal>0), group_key TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload)), PRIMARY KEY(session_id,ordinal), UNIQUE(session_id,group_key)) STRICT;
CREATE VIRTUAL TABLE context_search_fts USING fts5(session_id UNINDEXED,group_key UNINDEXED,content,tokenize='unicode61');
