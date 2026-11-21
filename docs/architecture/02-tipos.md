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
        Hello(heartbeat_interval_ms u64)
        HeartbeatAck
    }
    class ClientEvent {
        <<enumeration>>
        Heartbeat
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

- **`ServerEvent` e `ClientEvent`** são serializados como `{"t": "HELLO", "d": {...}}`, com
  testes em `crates/proto/src/ws.rs`. O servidor ainda não tem handler de WebSocket que os use.
- **IDs tipados** (`UserId`, `GuildId`, `ChannelId`, `MessageId`) saem da macro `id_type!`: cada
  um embrulha um `Uuid` com `From<Uuid>`, `Display` e `serde(transparent)`. Assim o compilador
  impede passar um `ChannelId` onde se espera um `UserId`. Ainda não são usados.

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

    main ..> Config : from_env e cors_layer
    main ..> AppState : cria com o PgPool
    main ..> routes : router(state)
    routes ..> health : GET /health
    health ..> AppState : usa state.db
    health ..> HealthResponse : devolve
```

## Client (`api-client`, `call-core`, `web`)

```mermaid
classDiagram
    direction LR

    class ApiClient {
        -String base_url
        -reqwest_Client http
        +new(base_url) ApiClient
        +base_url() str
        +health() Result~HealthResponse~
    }
    class ApiError {
        <<enumeration>>
        Network(reqwest_Error)
        Status(u16)
    }
    class ApiStatus {
        <<enumeration>>
        Checking
        Online(version, database)
        Offline(reason)
        +from_health(HealthResponse) ApiStatus
    }
    class App {
        <<component>>
        +signal ApiStatus
    }
    class status_view {
        <<function>>
        +status_view(ApiStatus) IntoView
    }
    class HealthResponse {
        <<proto>>
    }

    ApiClient ..> ApiError : em caso de falha
    ApiClient ..> HealthResponse : desserializa
    ApiStatus ..> HealthResponse : from_health
    App ..> ApiClient : new(API_URL) e health()
    App ..> ApiStatus : Checking para Online ou Offline
    App ..> status_view : renderiza
```

A constante `API_URL`, em `web/src/main.rs`, vem da variável `CALL_API_URL` na hora do build.
Sem ela, aponta para `http://127.0.0.1:3000`.
