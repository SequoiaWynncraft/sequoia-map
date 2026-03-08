CREATE TABLE claim_layouts (
    id               TEXT PRIMARY KEY,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    title            TEXT,
    document_version INTEGER NOT NULL,
    document         JSONB NOT NULL
);

CREATE INDEX idx_claim_layouts_created_at ON claim_layouts (created_at DESC);
