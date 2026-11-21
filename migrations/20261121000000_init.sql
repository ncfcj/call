-- Esquema inicial do MVP. IDs são UUIDv7 gerados pela aplicação (ordenáveis por tempo).

CREATE TABLE users (
    id            UUID PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE CHECK (char_length(username) BETWEEN 2 AND 32),
    display_name  TEXT NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 64),
    password_hash TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE guilds (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    owner_id    UUID NOT NULL REFERENCES users (id),
    invite_code TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE guild_members (
    guild_id  UUID NOT NULL REFERENCES guilds (id) ON DELETE CASCADE,
    user_id   UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id)
);
CREATE INDEX guild_members_user_idx ON guild_members (user_id);

CREATE TABLE channels (
    id         UUID PRIMARY KEY,
    guild_id   UUID NOT NULL REFERENCES guilds (id) ON DELETE CASCADE,
    name       TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
    kind       TEXT NOT NULL CHECK (kind IN ('text', 'voice')),
    position   INT  NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX channels_guild_idx ON channels (guild_id, position);

CREATE TABLE messages (
    id         UUID PRIMARY KEY,
    channel_id UUID NOT NULL REFERENCES channels (id) ON DELETE CASCADE,
    author_id  UUID REFERENCES users (id) ON DELETE SET NULL,
    content    TEXT NOT NULL CHECK (char_length(content) BETWEEN 1 AND 4000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    edited_at  TIMESTAMPTZ
);
-- Paginação do histórico: WHERE channel_id = $1 AND id < $before ORDER BY id DESC
CREATE INDEX messages_channel_idx ON messages (channel_id, id DESC);
