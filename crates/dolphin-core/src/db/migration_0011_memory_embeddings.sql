-- v11: cached embedding vectors for notes, so a note is embedded once per embedding model. Local to this device:
-- vectors depend on the model and are cheap to rebuild.
CREATE TABLE IF NOT EXISTS memory_embedding (
    memory_id TEXT NOT NULL,
    model     TEXT NOT NULL,
    text_hash TEXT NOT NULL,
    vector    BLOB NOT NULL,
    PRIMARY KEY (memory_id, model)
);
