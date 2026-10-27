-- solidway 主库初始化 schema（软件尚未发布，迁移合并为单一初始化脚本）。
-- 时间戳统一为 Unix 秒（INTEGER）。

CREATE TABLE users (
    id            TEXT PRIMARY KEY NOT NULL,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    disabled      INTEGER NOT NULL DEFAULT 0,
    display_name  TEXT,
    title         TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    archived_at   INTEGER,
    archived_by   TEXT
);

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);

CREATE TABLE routes (
    id                 TEXT PRIMARY KEY NOT NULL,
    owner_id           TEXT REFERENCES users (id) ON DELETE RESTRICT,
    name               TEXT NOT NULL,
    description        TEXT,
    enabled            INTEGER NOT NULL DEFAULT 1,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL,
    selection_strategy TEXT NOT NULL DEFAULT 'round_robin',
    selection_config   TEXT NOT NULL DEFAULT '{}',
    archived_at        INTEGER,
    archived_by        TEXT
);

CREATE INDEX routes_owner_id_idx ON routes (owner_id);

-- 活跃路由名称在同一 owner 内唯一；归档路由不占用名称。
CREATE UNIQUE INDEX routes_owner_active_name_uniq
    ON routes (owner_id, name)
    WHERE archived_at IS NULL;

CREATE TABLE provider_keys (
    id               TEXT PRIMARY KEY NOT NULL,
    route_id         TEXT NOT NULL REFERENCES routes (id) ON DELETE CASCADE,
    provider         TEXT NOT NULL,
    name             TEXT,
    base_url         TEXT,
    api_formats      TEXT NOT NULL DEFAULT '[]',
    auth_scheme      TEXT NOT NULL DEFAULT 'bearer',
    credential_kind  TEXT NOT NULL DEFAULT 'api_key'
                     CHECK (credential_kind IN ('api_key', 'google_service_account', 'aws_sigv4')),
    credential_ciphertext TEXT NOT NULL,
    credential_masked     TEXT NOT NULL,
    credential_hash       TEXT NOT NULL,
    options          TEXT,
    models           TEXT NOT NULL DEFAULT '[]',
    priority         INTEGER NOT NULL DEFAULT 1 CHECK (priority BETWEEN 1 AND 1000),
    status           TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive')),
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE INDEX provider_keys_route_id_idx ON provider_keys (route_id);

-- Gemini cachedContents / files 等上游资源的归属；后续请求必须回到同一 provider。
CREATE TABLE provider_resources (
    name        TEXT PRIMARY KEY NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('cached_content', 'file')),
    route_id    TEXT NOT NULL REFERENCES routes (id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL REFERENCES provider_keys (id) ON DELETE CASCADE,
    expires_at  INTEGER,
    created_at  INTEGER NOT NULL
);

CREATE INDEX provider_resources_route_kind_idx
    ON provider_resources (route_id, kind, created_at DESC);

CREATE TABLE provider_health (
    provider_id    TEXT PRIMARY KEY NOT NULL REFERENCES provider_keys (id) ON DELETE CASCADE,
    state          TEXT NOT NULL CHECK (state IN ('quarantined', 'retired')),
    reason         TEXT NOT NULL,
    cooldown_until INTEGER,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE api_keys (
    id           TEXT PRIMARY KEY NOT NULL,
    route_id     TEXT NOT NULL REFERENCES routes (id) ON DELETE CASCADE,
    name         TEXT,
    key_prefix   TEXT NOT NULL,
    key_masked   TEXT NOT NULL,
    key_hash     TEXT NOT NULL UNIQUE,
    status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive')),
    expires_at   INTEGER,
    last_used_at INTEGER,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX api_keys_route_id_idx ON api_keys (route_id);

CREATE TABLE settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    updated_by TEXT REFERENCES users (id) ON DELETE SET NULL
);

CREATE TABLE usage_bucket (
    granularity    TEXT NOT NULL CHECK (granularity IN ('hour', 'day')),
    bucket_ts      INTEGER NOT NULL,
    route_id       TEXT NOT NULL DEFAULT '',
    api_key_id     TEXT NOT NULL DEFAULT '',
    model          TEXT NOT NULL DEFAULT '',
    provider_id    TEXT NOT NULL DEFAULT '',
    owner_id       TEXT NOT NULL DEFAULT '',

    requests       INTEGER NOT NULL DEFAULT 0,
    completed      INTEGER NOT NULL DEFAULT 0,
    errors         INTEGER NOT NULL DEFAULT 0,
    client_errors  INTEGER NOT NULL DEFAULT 0,
    server_errors  INTEGER NOT NULL DEFAULT 0,
    aborted        INTEGER NOT NULL DEFAULT 0,
    stream_errors  INTEGER NOT NULL DEFAULT 0,
    usage_missing  INTEGER NOT NULL DEFAULT 0,
    failovers      INTEGER NOT NULL DEFAULT 0,

    input_tokens       INTEGER NOT NULL DEFAULT 0,
    output_tokens      INTEGER NOT NULL DEFAULT 0,
    cached_tokens      INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens   INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,

    latency_sum_ms  INTEGER NOT NULL DEFAULT 0,
    latency_count   INTEGER NOT NULL DEFAULT 0,
    ttft_sum_ms     INTEGER NOT NULL DEFAULT 0,
    ttft_count      INTEGER NOT NULL DEFAULT 0,
    duration_sum_ms INTEGER NOT NULL DEFAULT 0,
    duration_count  INTEGER NOT NULL DEFAULT 0,

    bytes_in       INTEGER NOT NULL DEFAULT 0,
    bytes_out      INTEGER NOT NULL DEFAULT 0,

    hist_0 INTEGER NOT NULL DEFAULT 0,
    hist_1 INTEGER NOT NULL DEFAULT 0,
    hist_2 INTEGER NOT NULL DEFAULT 0,
    hist_3 INTEGER NOT NULL DEFAULT 0,
    hist_4 INTEGER NOT NULL DEFAULT 0,
    hist_5 INTEGER NOT NULL DEFAULT 0,
    hist_6 INTEGER NOT NULL DEFAULT 0,
    hist_7 INTEGER NOT NULL DEFAULT 0,
    hist_8 INTEGER NOT NULL DEFAULT 0,
    hist_9 INTEGER NOT NULL DEFAULT 0,
    hist_10 INTEGER NOT NULL DEFAULT 0,

    UNIQUE (granularity, bucket_ts, route_id, api_key_id, model, provider_id, owner_id)
);

CREATE INDEX usage_bucket_time_idx ON usage_bucket (granularity, bucket_ts);
CREATE INDEX usage_bucket_model_idx ON usage_bucket (granularity, bucket_ts, model);
CREATE INDEX usage_bucket_apikey_idx ON usage_bucket (granularity, bucket_ts, api_key_id);
CREATE INDEX usage_bucket_owner_idx ON usage_bucket (owner_id, granularity, bucket_ts);

CREATE TABLE usage_attempt_bucket (
    granularity    TEXT NOT NULL CHECK (granularity IN ('hour', 'day')),
    bucket_ts      INTEGER NOT NULL,
    route_id       TEXT NOT NULL DEFAULT '',
    provider_id    TEXT NOT NULL DEFAULT '',
    model          TEXT NOT NULL DEFAULT '',
    attempts       INTEGER NOT NULL DEFAULT 0,
    success        INTEGER NOT NULL DEFAULT 0,
    errors         INTEGER NOT NULL DEFAULT 0,
    status_4xx     INTEGER NOT NULL DEFAULT 0,
    status_5xx     INTEGER NOT NULL DEFAULT 0,
    latency_sum_ms INTEGER NOT NULL DEFAULT 0,
    latency_count  INTEGER NOT NULL DEFAULT 0,
    UNIQUE (granularity, bucket_ts, route_id, provider_id, model)
);

CREATE INDEX usage_attempt_bucket_time_idx
    ON usage_attempt_bucket (granularity, bucket_ts);

CREATE TABLE runtime_snapshot (
    id         TEXT PRIMARY KEY CHECK (id = 'current'),
    payload    TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE provider_incident (
    id           TEXT PRIMARY KEY NOT NULL,
    provider_id  TEXT NOT NULL,
    route_id     TEXT NOT NULL,
    owner_id     TEXT NOT NULL DEFAULT '',
    kind         TEXT NOT NULL,
    severity     TEXT NOT NULL CHECK (severity IN ('transient', 'degraded', 'manual_action')),
    state        TEXT NOT NULL CHECK (state IN ('open', 'closed')),
    started_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    ended_at     INTEGER,
    attempts     INTEGER NOT NULL DEFAULT 1,
    last_status  INTEGER,
    last_error   TEXT,
    resolved_by  TEXT,
    notified_at  INTEGER
);

CREATE INDEX provider_incident_open_idx
    ON provider_incident (provider_id, state);
CREATE INDEX provider_incident_started_idx
    ON provider_incident (started_at);
CREATE INDEX provider_incident_route_idx
    ON provider_incident (route_id, started_at);

-- 轻资源删除 / 历史身份回退。无外键：必须比活资源活得更久。
CREATE TABLE resource_tombstone (
    kind         TEXT NOT NULL CHECK (kind IN ('user', 'route', 'provider_key', 'api_key')),
    resource_id  TEXT NOT NULL,
    parent_id    TEXT,
    owner_id     TEXT,
    owner_name   TEXT,
    label        TEXT,
    provider     TEXT,
    key_prefix   TEXT,
    deleted_at   INTEGER NOT NULL,
    deleted_by   TEXT,
    reason       TEXT,
    metadata     TEXT,
    PRIMARY KEY (kind, resource_id)
);

CREATE INDEX resource_tombstone_owner_idx
    ON resource_tombstone (kind, owner_id);
CREATE INDEX resource_tombstone_parent_idx
    ON resource_tombstone (kind, parent_id);
CREATE INDEX resource_tombstone_deleted_at_idx
    ON resource_tombstone (deleted_at);

-- 身份视图：活行（含归档）与墓碑的并集。
CREATE VIEW route_identity AS
SELECT r.id AS id,
       r.name AS label,
       r.owner_id AS owner_id,
       COALESCE(u.display_name, u.username) AS owner_name,
       r.archived_at AS archived_at,
       0 AS tombstone
FROM routes r
LEFT JOIN users u ON u.id = r.owner_id
UNION ALL
SELECT t.resource_id, t.label, t.owner_id, t.owner_name, NULL, 1
FROM resource_tombstone t
WHERE t.kind = 'route';

CREATE VIEW provider_identity AS
SELECT k.id AS id,
       k.route_id AS route_id,
       COALESCE(k.name, k.provider) AS label,
       k.provider AS provider,
       r.owner_id AS owner_id,
       COALESCE(u.display_name, u.username) AS owner_name,
       r.archived_at AS archived_at,
       0 AS tombstone
FROM provider_keys k
JOIN routes r ON r.id = k.route_id
LEFT JOIN users u ON u.id = r.owner_id
UNION ALL
SELECT t.resource_id, t.parent_id, t.label, t.provider,
       t.owner_id, t.owner_name, NULL, 1
FROM resource_tombstone t
WHERE t.kind = 'provider_key';

CREATE VIEW api_key_identity AS
SELECT k.id AS id,
       k.route_id AS route_id,
       COALESCE(k.name, k.key_prefix) AS label,
       k.key_prefix AS key_prefix,
       r.owner_id AS owner_id,
       COALESCE(u.display_name, u.username) AS owner_name,
       r.archived_at AS archived_at,
       0 AS tombstone
FROM api_keys k
JOIN routes r ON r.id = k.route_id
LEFT JOIN users u ON u.id = r.owner_id
UNION ALL
SELECT t.resource_id, t.parent_id, t.label, t.key_prefix,
       t.owner_id, t.owner_name, NULL, 1
FROM resource_tombstone t
WHERE t.kind = 'api_key';

CREATE VIEW user_identity AS
SELECT u.id AS id,
       COALESCE(u.display_name, u.username) AS label,
       u.username AS username,
       u.archived_at AS archived_at,
       0 AS tombstone
FROM users u
UNION ALL
SELECT t.resource_id, t.label, t.label, NULL, 1
FROM resource_tombstone t
WHERE t.kind = 'user';
