-- Initial schema for ASURA.
--
-- Three logical databases in production (auth, game, content). This file is the union, written so it
-- can be applied to one database during development and split later without rewriting statements.
-- Parallel in AzerothCore: Login / Character / World databases, each with its own connection pool.
--
-- The registry and pool tables are the coordination channel between the auth service and the world
-- servers (docs/02-architecture.md). They exist because placement must be atomic across processes.

CREATE TABLE IF NOT EXISTS schema_version (
    version     int         PRIMARY KEY,
    applied_at  timestamptz NOT NULL DEFAULT now()
);

-- ---------------------------------------------------------------- auth domain

CREATE TABLE IF NOT EXISTS account (
    id          uuid        PRIMARY KEY,
    username    text        NOT NULL UNIQUE,
    -- Login token the world server validates a reconnecting client against.
    token       uuid,
    token_at    timestamptz,
    last_ip     inet,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS session (
    id          uuid        PRIMARY KEY,
    account_id  uuid        NOT NULL REFERENCES account (id) ON DELETE CASCADE,
    char_id     uuid,
    world_id    int,
    started_at  timestamptz NOT NULL DEFAULT now(),
    -- The newest session for an account wins; older ones are closed, never honoured
    -- (docs/05-gw2-lessons.md, rule 1).
    ended_at    timestamptz
);

CREATE INDEX IF NOT EXISTS session_by_account ON session (account_id) WHERE ended_at IS NULL;

-- ---------------------------------------------------------------- registry

CREATE TABLE IF NOT EXISTS world (
    id            int         PRIMARY KEY,
    name          text        NOT NULL,
    address       text        NOT NULL,
    port          int         NOT NULL,
    build         text        NOT NULL,
    draining      bool        NOT NULL DEFAULT false,
    population    int         NOT NULL DEFAULT 0,
    heartbeat_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS world_pool (
    world_id   int  NOT NULL REFERENCES world (id) ON DELETE CASCADE,
    map_id     int  NOT NULL,
    capacity   int  NOT NULL CHECK (capacity >= 0),
    leased     int  NOT NULL DEFAULT 0 CHECK (leased >= 0),
    PRIMARY KEY (world_id, map_id),
    CHECK (leased <= capacity)
);

-- ---------------------------------------------------------------- game domain

CREATE TABLE IF NOT EXISTS character (
    id          uuid        PRIMARY KEY,
    account_id  uuid        NOT NULL REFERENCES account (id) ON DELETE CASCADE,
    name        text        NOT NULL,
    map_id      int         NOT NULL,
    pos_x       real        NOT NULL DEFAULT 0,
    pos_y       real        NOT NULL DEFAULT 0,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS character_by_account ON character (account_id);

-- ---------------------------------------------------------------- content domain

CREATE TABLE IF NOT EXISTS map_config (
    map_id        int     PRIMARY KEY,
    map_type      int     NOT NULL,          -- 0 = outpost/town, 1 = explorable
    map_file_id   bigint  NOT NULL,
    spawn_count   int     NOT NULL DEFAULT 0
);
