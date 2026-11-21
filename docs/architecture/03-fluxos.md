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

## `GET /health`

Usado para conferir se a API e o banco estão de pé (e, na Fase 3, pelo monitoramento). O client
não chama mais essa rota: o fluxo dele começa no refresh da sessão ([08-client.md](08-client.md)).

```mermaid
sequenceDiagram
    participant C as Quem chama
    participant S as call-server
    participant H as health()
    participant PG as Postgres

    C->>S: GET /health
    S->>H: State(AppState)
    H->>PG: SELECT 1
    alt banco responde
        H-->>C: 200 {status: "ok", version, database: true}
    else banco fora
        H-->>C: 200 {status: "degraded", version, database: false}
    end
```
