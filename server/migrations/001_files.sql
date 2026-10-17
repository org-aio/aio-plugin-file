CREATE TABLE file_objects (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    original_name TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes BETWEEN 0 AND 104857600),
    sha256 TEXT,
    uploaded_by TEXT NOT NULL,
    image_token TEXT UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '24 hours',
    state TEXT NOT NULL DEFAULT 'uploading' CHECK (state IN ('uploading', 'ready', 'deleting')),
    cleanup_after TIMESTAMPTZ,
    CHECK (sha256 IS NULL OR sha256 ~ '^[a-f0-9]{64}$'),
    CHECK (state <> 'ready' OR sha256 IS NOT NULL),
    CHECK (state <> 'deleting' OR cleanup_after IS NOT NULL),
    UNIQUE (tenant_id, id)
);
CREATE INDEX file_objects_tenant_created ON file_objects (tenant_id, created_at DESC, id);
CREATE INDEX file_objects_cleanup ON file_objects (cleanup_after) WHERE state='deleting';
CREATE TABLE file_chunks (
    tenant_id TEXT NOT NULL,
    file_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL CHECK (chunk_index BETWEEN 0 AND 24),
    sha256 TEXT NOT NULL CHECK (sha256 ~ '^[a-f0-9]{64}$'),
    stored BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (tenant_id, file_id, chunk_index),
    FOREIGN KEY (tenant_id, file_id) REFERENCES file_objects (tenant_id, id) ON DELETE CASCADE
);
