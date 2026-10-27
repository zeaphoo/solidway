-- 明细库（usage_detail.db）：只放逐条请求明细，与主库分离。

CREATE TABLE usage_request_detail (
    request_id         TEXT PRIMARY KEY NOT NULL,
    at                 INTEGER NOT NULL,
    route_id           TEXT NOT NULL,
    api_key_id         TEXT NOT NULL,
    owner_id           TEXT NOT NULL DEFAULT '',
    model              TEXT NOT NULL DEFAULT '',
    provider_id        TEXT NOT NULL DEFAULT '',
    api_format         TEXT NOT NULL DEFAULT '',
    is_stream          INTEGER NOT NULL DEFAULT 0,
    status             INTEGER NOT NULL,
    attempts           INTEGER NOT NULL DEFAULT 0,
    failovers          INTEGER NOT NULL DEFAULT 0,
    latency_ms         INTEGER NOT NULL DEFAULT 0,
    ttft_ms            INTEGER,
    bytes_in           INTEGER NOT NULL DEFAULT 0,
    bytes_out          INTEGER NOT NULL DEFAULT 0,
    attempts_json      TEXT NOT NULL DEFAULT '[]',
    outcome            TEXT NOT NULL DEFAULT 'completed',
    duration_ms        INTEGER NOT NULL DEFAULT 0,
    input_tokens       INTEGER NOT NULL DEFAULT 0,
    output_tokens      INTEGER NOT NULL DEFAULT 0,
    cached_tokens      INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens   INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX usage_request_detail_at_idx ON usage_request_detail (at);
CREATE INDEX usage_request_detail_route_at_idx
    ON usage_request_detail (route_id, at);
CREATE INDEX usage_request_detail_key_at_idx
    ON usage_request_detail (api_key_id, at);
CREATE INDEX usage_request_detail_owner_at_idx
    ON usage_request_detail (owner_id, at);
