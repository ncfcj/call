# Autenticação

Implementada no servidor na Fase 1. O client ainda não usa.

- **Access token:** JWT HS256 válido por 15 min, com `sub` = id do usuário. Vai no corpo da
  resposta e volta como `Authorization: Bearer`.
- **Refresh token:** 256 bits aleatórios, válido por 30 dias, num cookie `HttpOnly`, `SameSite=Lax`
  e `Path=/auth`. O banco guarda só o SHA-256 dele.
- **Senha:** Argon2id, executado fora do runtime async (`spawn_blocking`).

## Módulos do servidor

```mermaid
classDiagram
    direction LR

    class routes_auth {
        <<handlers>>
        +register(Json~RegisterRequest~)
        +login(Json~LoginRequest~)
        +refresh(CookieJar)
        +logout(CookieJar)
    }
    class routes_users {
        <<handlers>>
        +me(AuthUser) Json~User~
        +find_user(db, UserId) Option~User~
    }
    class password {
        <<module>>
        +hash(String) String
        +verify(String, Option~String~) bool
    }
    class JwtKeys {
        +new(secret) JwtKeys
        +issue(UserId) String
        +verify(token) Option~UserId~
    }
    class refresh {
        <<module>>
        +start_session(db, UserId) String
        +rotate(db, token) (UserId, String)
        +end_session(db, token)
    }
    class cookie {
        <<module>>
        +set_refresh(jar, token, secure)
        +clear_refresh(jar)
        +read_refresh(jar)
    }
    class AuthUser {
        <<extractor>>
        +UserId 0
    }
    class AppError {
        <<enumeration>>
        Validation
        UsernameTaken
        InvalidCredentials
        Unauthorized
        NotFound
        Internal
    }
    class AppState {
        +PgPool db
        +Arc~JwtKeys~ jwt
        +bool cookie_secure
    }

    routes_auth ..> password
    routes_auth ..> refresh
    routes_auth ..> cookie
    routes_auth ..> JwtKeys : issue
    routes_auth ..> routes_users : find_user
    routes_users ..> AuthUser
    AuthUser ..> JwtKeys : verify
    AuthUser ..> AppState
    AppState --> JwtKeys
    routes_auth ..> AppError
    AuthUser ..> AppError : Unauthorized
```

O `AppError` vira uma resposta `proto::ErrorResponse { code, message }`. Os erros internos vão
para o log, e o client recebe só "erro interno do servidor".

## Cadastro e login

```mermaid
sequenceDiagram
    autonumber
    participant C as Client
    participant A as routes::auth
    participant P as password (Argon2)
    participant DB as Postgres

    alt register
        C->>A: POST /auth/register {username, display_name, password}
        A->>A: normalize_username + validate_* (proto)
        A->>P: hash(password)
        A->>DB: INSERT users
        Note over A,DB: violação de UNIQUE → 409 username_taken
    else login
        C->>A: POST /auth/login {username, password}
        A->>DB: SELECT users WHERE username
        A->>P: verify(password, hash ou None)
        Note over A,P: sem usuário, compara com um hash fictício<br/>(mesmo tempo de resposta) → 401 invalid_credentials
    end
    A->>DB: INSERT refresh_tokens (família nova)
    A-->>C: 201/200 {access_token, expires_in, user}<br/>Set-Cookie: refresh_token=...
```

## Refresh com rotação e detecção de reuso

```mermaid
sequenceDiagram
    autonumber
    participant C as Client
    participant A as routes::auth::refresh
    participant DB as Postgres

    C->>A: POST /auth/refresh (Cookie: refresh_token)
    A->>DB: SELECT ... WHERE token_hash FOR UPDATE
    alt não existe, revogado ou expirado
        A-->>C: 401 unauthorized
    else já usado há menos de 10 s
        Note over A: corrida entre duas abas: 401 sem revogar<br/>e sem apagar o cookie novo
        A-->>C: 401 unauthorized
    else já usado há mais de 10 s
        Note over A,DB: token velho voltou: possível roubo
        A->>DB: UPDATE revoked_at em toda a família
        A-->>C: 401 unauthorized
    else válido
        A->>DB: UPDATE used_at = now()
        A->>DB: INSERT token novo na mesma família
        A-->>C: 200 {access_token, ...}<br/>Set-Cookie: refresh_token=novo
    end
```

## Estados de um refresh token

```mermaid
stateDiagram-v2
    [*] --> Ativo: login, cadastro ou rotação
    Ativo --> Usado: refresh (emite o próximo)
    Ativo --> Revogado: logout ou reuso na família
    Ativo --> Expirado: 30 dias
    Usado --> Revogado: reapresentado depois de 10 s
    Revogado --> [*]
    Expirado --> [*]
```
