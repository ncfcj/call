# Banco de dados

Este é o esquema da migration `migrations/20261121000000_init.sql`. As tabelas já são criadas na
subida do servidor, mas nenhum código as usa ainda: elas entram na Fase 1. Os IDs são UUIDv7
gerados pela aplicação e correspondem aos tipos `UserId`, `GuildId`, `ChannelId` e `MessageId`
do `proto`.

```mermaid
erDiagram
    users ||--o{ guilds : "é dono (owner_id)"
    users ||--o{ guild_members : participa
    guilds ||--o{ guild_members : "tem (CASCADE)"
    guilds ||--o{ channels : "tem (CASCADE)"
    channels ||--o{ messages : "tem (CASCADE)"
    users |o--o{ messages : "escreve (SET NULL)"

    users {
        UUID id PK
        TEXT username UK "2 a 32 caracteres"
        TEXT display_name "1 a 64 caracteres"
        TEXT password_hash "Argon2 na Fase 1"
        TIMESTAMPTZ created_at
    }
    guilds {
        UUID id PK
        TEXT name "1 a 100 caracteres"
        UUID owner_id FK
        TEXT invite_code UK
        TIMESTAMPTZ created_at
    }
    guild_members {
        UUID guild_id PK,FK
        UUID user_id PK,FK
        TIMESTAMPTZ joined_at
    }
    channels {
        UUID id PK
        UUID guild_id FK
        TEXT name "1 a 100 caracteres"
        TEXT kind "text ou voice"
        INT position
        TIMESTAMPTZ created_at
    }
    messages {
        UUID id PK
        UUID channel_id FK
        UUID author_id FK "nulo se o autor for apagado"
        TEXT content "1 a 4000 caracteres"
        TIMESTAMPTZ created_at
        TIMESTAMPTZ edited_at
    }
```

## Índices

| Índice | Para que serve |
| --- | --- |
| `guild_members_user_idx (user_id)` | Listar os servidores de um usuário |
| `channels_guild_idx (guild_id, position)` | Listar os canais de um servidor em ordem |
| `messages_channel_idx (channel_id, id DESC)` | Paginação `?before=<id>` do histórico. Funciona porque o UUIDv7 é ordenável por tempo |

## Observações

- Apagar um servidor (guild) apaga em cascata os membros, os canais e as mensagens. Apagar um
  usuário mantém as mensagens dele, com `author_id` nulo.
- Apagar o usuário dono de um servidor falha, porque `guilds.owner_id` não tem `ON DELETE`.
  Defina o comportamento na Fase 1 (transferir o servidor ou bloquear a exclusão).
- Ainda não há tabela para o refresh token rotativo da Fase 1. Ela precisa de uma migration nova.
