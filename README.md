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

## Nota sobre o OneDrive

Esta pasta é sincronizada pelo OneDrive. A pasta `target/` chega a vários GB e o OneDrive
tenta sincronizá-la, o que deixa o build lento e pode travar arquivos. Aponte o diretório de
build para fora do OneDrive, uma vez, para o seu usuário:

```powershell
setx CARGO_TARGET_DIR "$env:USERPROFILE\.cargo-target"
```

## Qualidade

```powershell
cargo fmt --all
cargo clippy -p proto -p call-server --all-targets -- -D warnings
cargo clippy -p proto -p api-client -p call-core -p voice -p web --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo test -p proto -p call-server
```
