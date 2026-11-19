CREATE VIRTUAL TABLE memory_fts USING fts5(relative_path, title, body, short_terms, tokenize='trigram');
