-- Refresh tokens rotativos. Só o hash SHA-256 do token é guardado.
-- Cada login abre uma "família"; cada refresh marca o token atual como usado e emite outro
-- na mesma família. Se um token já usado voltar (roubo), a família inteira é revogada.

CREATE TABLE refresh_tokens (
    id         UUID PRIMARY KEY,
    user_id    UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    family_id  UUID NOT NULL,
    token_hash BYTEA NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at    TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);
CREATE INDEX refresh_tokens_family_idx ON refresh_tokens (family_id);
CREATE INDEX refresh_tokens_user_idx ON refresh_tokens (user_id);
