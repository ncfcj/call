# Call

MVP de chat de texto e voz estilo Discord, num monorepo Rust (um workspace Cargo).
O servidor é o *control plane* (auth, servidores, canais, chat via WebSocket e tokens de voz);
o áudio vai direto do client para o LiveKit. Veja o `Plano de MVP.md`.

## Estrutura

| Caminho | Conteúdo | Alvo |
| --- | --- | --- |
| `crates/proto` | Contrato REST e eventos WS, compartilhado entre servidor e client | Nativo e wasm32 |
| `crates/api-client` | Chamadas REST (`reqwest` sobre o `fetch` do navegador) | wasm32 |
| `crates/core` | Estado do client sem dependência de UI | wasm32 |
| `crates/voice` | Ponte `wasm-bindgen` com o `livekit-client` (Fase 2) | wasm32 |
| `server/` | Binário `call-server` (axum + tokio + sqlx) | Nativo |
| `web/` | UI Leptos (CSR) e build Trunk | wasm32 |
| `migrations/` | Migrations SQL, aplicadas automaticamente na subida do servidor | |
| `docker-compose.yml` | Postgres 17 e LiveKit em modo dev | |

O `src-tauri/` (casca desktop para Windows) entra na Fase 3.

## Rodando localmente

Pré-requisitos: Rust estável (o `rust-toolchain.toml` instala o alvo `wasm32-unknown-unknown`),
Docker Desktop e o Trunk (`cargo install trunk --locked`).

Servidor:

```powershell
copy .env.example .env
docker compose up -d
cargo run -p call-server
```

Teste: `curl http://127.0.0.1:3000/health` deve responder `{"status":"ok",...,"database":true}`.

Client, em outro terminal:

```powershell
cd web
trunk serve --open
```

A página abre em `http://localhost:8080` e mostra se a API e o banco estão online.
Para apontar para outra API no build: `$env:CALL_API_URL="https://api.exemplo.com"; trunk build --release`.

## Perfis de build

O `release` padrão otimiza o servidor para velocidade. O client usa o perfil `wasm-release`
(`opt-level = "z"`, LTO), escolhido pelo `data-cargo-profile` em `web/index.html`.

## Qualidade

```powershell
cargo fmt --all
cargo clippy -p proto -p call-server --all-targets -- -D warnings
cargo clippy -p proto -p api-client -p call-core -p voice -p web --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo test -p proto -p call-server
```

Os testes de integração do servidor (`server/tests/`) precisam do Postgres do `docker compose`
ligado e de `DATABASE_URL` no ambiente: cada teste cria e apaga um banco próprio.

## API

| Rota | Auth | O que faz |
| --- | --- | --- |
| `GET /health` | | Status da API e do banco |
| `POST /auth/register` | | Cria a conta e abre a sessão (201) |
| `POST /auth/login` | | Abre a sessão |
| `POST /auth/refresh` | cookie | Troca o refresh token (rotação) e emite um access token novo |
| `POST /auth/logout` | cookie | Revoga a sessão e apaga o cookie (204) |
| `GET /users/@me` | Bearer | Usuário logado |

O access token (JWT, 15 min) vai no corpo da resposta e é enviado como
`Authorization: Bearer`. O refresh token (30 dias) fica num cookie `HttpOnly` com `Path=/auth`.
Erros respondem `{"code": "...", "message": "..."}`, com os códigos de `proto::ErrorCode`.

