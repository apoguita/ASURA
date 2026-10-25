-- Initial schema for ASURA.
--
-- MySQL 8.0.16 or later, matching AzerothCore's vendor (docs/adr/0008-database-vendor-mysql.md).
-- Three logical databases in production -- auth, game, content -- mirroring AzerothCore's Login /
-- Character / World split. This file is their union so it can be applied to one schema during
-- development and split later without rewriting statements.
--
-- The registry and pool tables are the coordination channel between the auth service and the world
-- servers (docs/02-architecture.md): placement must be atomic across processes, and the database is
-- the only arbiter both kinds of program agree on.
--
-- Type notes, so the mapping is not folklore:
--   * UUIDs are CHAR(36): the client protocol carries 128-bit identifiers and GWPS stored them as
--     text. BINARY(16) is the optimisation if indexing ever makes it worth the unreadability.
--   * There is no timezone-aware type; every DATETIME is UTC by convention.
--   * CHECK constraints are enforced from MySQL 8.0.16 onward, and we rely on them.
--   * MySQL has no partial indexes, so "active sessions per account" uses a composite index
--     instead of a WHERE clause on the index.

CREATE TABLE IF NOT EXISTS schema_version (
    version     INT          NOT NULL PRIMARY KEY,
    applied_at  DATETIME(6)  NOT NULL DEFAULT CURRENT_TIMESTAMP(6)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ---------------------------------------------------------------- auth domain

CREATE TABLE IF NOT EXISTS account (
    id          CHAR(36)     NOT NULL PRIMARY KEY,
    username    VARCHAR(255) NOT NULL,
    token       CHAR(36)     NULL,
    token_at    DATETIME(6)  NULL,
    last_ip     VARCHAR(45)  NULL,
    created_at  DATETIME(6)  NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    UNIQUE KEY account_username (username)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS session (
    id          CHAR(36)     NOT NULL PRIMARY KEY,
    account_id  CHAR(36)     NOT NULL,
    char_id     CHAR(36)     NULL,
    world_id    INT          NULL,
    started_at  DATETIME(6)  NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
    ended_at    DATETIME(6)  NULL,
    CONSTRAINT session_account FOREIGN KEY (account_id) REFERENCES account (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- Replaces a partial index (MySQL has none). Queries filter on ended_at being set; the leading
-- account_id keeps "all sessions for this account" cheap, which is the only shape we ask for.
CREATE INDEX session_by_account ON session (account_id, ended_at);

-- ---------------------------------------------------------------- registry

CREATE TABLE IF NOT EXISTS world (
    id            INT         NOT NULL PRIMARY KEY,
    name          VARCHAR(64) NOT NULL,
    address       VARCHAR(255) NOT NULL,
    port          INT         NOT NULL,
    build         VARCHAR(64) NOT NULL,
    draining      TINYINT(1)  NOT NULL DEFAULT 0,
    population    INT         NOT NULL DEFAULT 0,
    heartbeat_at  DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS world_pool (
    world_id   INT  NOT NULL,
    map_id     INT  NOT NULL,
    capacity   INT  NOT NULL,
    leased     INT  NOT NULL DEFAULT 0,
    PRIMARY KEY (world_id, map_id),
    CONSTRAINT world_pool_capacity_positive CHECK (capacity >= 0),
    CONSTRAINT world_pool_leased_positive   CHECK (leased >= 0),
    CONSTRAINT world_pool_never_oversold    CHECK (leased <= capacity),
    CONSTRAINT world_pool_world FOREIGN KEY (world_id) REFERENCES world (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- ---------------------------------------------------------------- game domain

CREATE TABLE IF NOT EXISTS characters (
    id          CHAR(36)     NOT NULL PRIMARY KEY,
    account_id  CHAR(36)     NOT NULL,
    name        VARCHAR(255) NOT NULL,
    map_id      INT          NOT NULL,
    pos_x       FLOAT        NOT NULL DEFAULT 0,
    pos_y       FLOAT        NOT NULL DEFAULT 0,
    updated_at  DATETIME(6)  NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
    CONSTRAINT characters_account FOREIGN KEY (account_id) REFERENCES account (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE INDEX characters_by_account ON characters (account_id);

-- ---------------------------------------------------------------- content domain

CREATE TABLE IF NOT EXISTS map_config (
    map_id        INT     NOT NULL PRIMARY KEY,
    map_type      INT     NOT NULL,
    map_file_id   BIGINT  NOT NULL,
    spawn_count   INT     NOT NULL DEFAULT 0
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
