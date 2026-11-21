# Fluxos

## Subida do servidor

```mermaid
sequenceDiagram
    autonumber
    participant M as main()
    participant C as Config
    participant PG as Postgres
    participant R as routes::router
    participant L as TcpListener

    M->>M: dotenvy::dotenv() e tracing
    M->>C: Config::from_env()
    C-->>M: database_url, bind_addr, web_origins
    M->>PG: PgPoolOptions::connect (até 5 conexões, timeout de 10 s)
    M->>PG: sqlx::migrate!("../migrations").run()
    M->>R: router(AppState { db })
    R-->>M: Router com cors_layer() e TraceLayer
    M->>L: bind(bind_addr)
    M->>M: axum::serve(...) até o Ctrl+C (shutdown_signal)
```

Se faltar o `DATABASE_URL`, se o Postgres não responder ou se uma migration falhar, o processo
termina com erro antes de abrir a porta.

## `GET /health` de ponta a ponta

É o único fluxo completo hoje, e já percorre o caminho que a Fase 1 vai usar: Leptos,
`api-client`, CORS, axum, Postgres, tipos do `proto` e de volta à UI.

```mermaid
sequenceDiagram
    autonumber
    participant UI as App (Leptos)
    participant AC as ApiClient
    participant S as call-server
    participant H as health()
    participant PG as Postgres

    UI->>UI: signal(ApiStatus::Checking)
    UI->>AC: spawn_local(api.health())
    AC->>S: GET {API_URL}/health (fetch do navegador)
    S->>S: CorsLayer confere a Origin (WEB_ORIGIN)
    S->>H: State(AppState)
    H->>PG: SELECT 1
    alt banco responde
        PG-->>H: ok
        H-->>AC: 200 {status: "ok", version, database: true}
    else banco fora
        H-->>AC: 200 {status: "degraded", version, database: false}
    end
    alt resposta 2xx
        AC-->>UI: Ok(HealthResponse)
        UI->>UI: ApiStatus::from_health vira Online
    else erro de rede ou status fora de 2xx
        AC-->>UI: Err(ApiError)
        UI->>UI: ApiStatus::Offline { reason }
    end
    UI->>UI: status_view() redesenha
```

## Estados da tela

```mermaid
stateDiagram-v2
    [*] --> Checking
    Checking --> OnlineComBanco: database = true
    Checking --> OnlineSemBanco: database = false
    Checking --> Offline: ApiError (rede ou HTTP)
    OnlineComBanco: Online, database true
    OnlineSemBanco: Online, database false
    Offline: Offline, com o motivo
```

| Estado | Texto na tela |
| --- | --- |
| `Checking` | "Verificando a API..." |
| `Online { database: true }` | "API online (vX), banco conectado" |
| `Online { database: false }` | "API online (vX), mas sem banco" |
| `Offline { reason }` | "API indisponível: ..." |

Hoje a checagem roda uma vez só, quando a página monta. Não há nova tentativa.
