# Tipos e funções

## Contrato (`proto`)

```mermaid
classDiagram
    direction LR

    class HealthResponse {
        +HealthStatus status
        +String version
        +bool database
    }
    class HealthStatus {
        <<enumeration>>
        Ok
        Degraded
    }
    HealthResponse --> HealthStatus

    class ServerEvent {
        <<enumeration>>
        Hello(heartbeat_interval_ms, user_id)
        HeartbeatAck
        Subscribed(guild_id, online)
        MessageCreate(Message)
        TypingStart(guild_id, channel_id, user_id)
        PresenceUpdate(guild_id, user_id, status)
        Error(code, message)
    }
    class ClientEvent {
        <<enumeration>>
        Heartbeat
        Subscribe(guild_id)
        TypingStart(channel_id)
    }

    class UserId {
        +Uuid 0
    }
    class GuildId {
        +Uuid 0
    }
    class ChannelId {
        +Uuid 0
    }
    class MessageId {
        +Uuid 0
    }
```

- **`ServerEvent` e `ClientEvent`** são serializados como `{"t": "HELLO", "d": {...}}`. Quando cada um
  é enviado está em [07-websocket.md](07-websocket.md).
- **IDs tipados** (`UserId`, `GuildId`, `ChannelId`, `MessageId`) saem da macro `id_type!`: cada
  um embrulha um `Uuid` com `From<Uuid>`, `Display` e `serde(transparent)`. Assim o compilador
  impede passar um `ChannelId` onde se espera um `UserId`.
- **Auth, servidores, canais e mensagens:** os tipos estão em [05-auth.md](05-auth.md) e
  [06-servidores.md](06-servidores.md).

## Servidor (`call-server`)

```mermaid
classDiagram
    direction LR

    class Config {
        +String database_url
        +SocketAddr bind_addr
        +Vec~HeaderValue~ web_origins
        +from_env() Result~Config~
        +cors_layer() CorsLayer
    }
    class AppState {
        +PgPool db
    }
    class main {
        <<module>>
        +main() Result
        -shutdown_signal()
    }
    class routes {
        <<module>>
        +router(AppState) Router
    }
    class health {
        <<handler>>
        +health(State~AppState~) Json~HealthResponse~
    }
    class HealthResponse {
        <<proto>>
    }
    class auth {
        <<module>>
        +register()
        +login()
        +refresh()
        +logout()
        +me()
    }

    main ..> Config : from_env e cors_layer
    main ..> AppState : cria com o PgPool
    main ..> routes : router(state)
    routes ..> health : GET /health
    health ..> AppState : usa state.db
    health ..> HealthResponse : devolve
    routes ..> auth : /auth/* e /users/@me
    class guilds_channels_messages {
        <<module>>
    }
    routes ..> guilds_channels_messages : /guilds, /channels, /invites
```

Os módulos de autenticação estão detalhados em [05-auth.md](05-auth.md), e os de servidores,
canais e mensagens em [06-servidores.md](06-servidores.md). O `Config` também lê
`JWT_SECRET` e `COOKIE_SECURE`, e o `AppState` guarda as chaves JWT e essa flag.

## Client (`api-client`, `call-core`, `web`)

Os tipos e o fluxo do client estão em [08-client.md](08-client.md).
