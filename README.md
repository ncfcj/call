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

A página abre em `http://localhost:8080`, na tela de entrada. O client chama a API no mesmo host
da página, na porta 3000. Abra por `localhost` e não por `127.0.0.1`: se o host for diferente
do da API, o navegador não manda o cookie da sessão.
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

### Teste funcional de cada etapa

Cada etapa tem um script em `scripts/` que passa pelo fluxo real contra o servidor rodando
(`cargo run -p call-server`) e imprime OK ou FALHOU por passo:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\teste-servidores.ps1
```

| Script | Etapa |
| --- | --- |
| `teste-servidores.ps1` | Servidores, canais, convite, permissões de dono e histórico de mensagens |
| `teste-websocket.ps1` | Ticket, heartbeat, inscrição, presença, digitação e mensagem em tempo real com três conexões |
| `teste-client.ps1 -Convite <código>` | Um robô entra no seu servidor e conversa com você no navegador (veja abaixo) |
| `teste-voz-token.ps1` | Token de voz: emissão, validação no próprio LiveKit e recusas |
| (manual) | Chamada de voz entre duas janelas (veja abaixo) |

Os dados criados ficam no banco local, e o script mostra no fim o login de um usuário de teste.

**Client no navegador.** Com o servidor e o `trunk serve` rodando:

1. Crie uma conta, crie um servidor e copie o código de convite que aparece abaixo do nome dele.
2. Rode `powershell -ExecutionPolicy Bypass -File scripts	este-client.ps1 -Convite <código>`.
   O robô aparece online, digita e manda uma mensagem. Ele responde cada mensagem sua.
3. Pare o servidor (Ctrl+C), espere alguns segundos e suba de novo. O rodapé mostra
   "Sem conexão. Tentando em N s" e volta para "Conectado" sozinho, e nenhuma mensagem some.
4. Recarregue a página: a sessão continua. Clique em "Sair" e recarregue: volta para a entrada.

Para conversar entre duas pessoas de verdade, use dois navegadores (ou uma janela anônima),
porque a sessão fica num cookie por navegador.

**Chamada de voz entre duas janelas.** Antes, ponha em `LIVEKIT_NODE_IP` (no `.env`) o IP desta
máquina na rede local e rode `docker compose up -d` de novo. Com o padrão `127.0.0.1`, só o
Chrome conecta: o Firefox recusa endereços de loopback no WebRTC e fica em "Conectando à voz...".
Use fones de ouvido: as duas janelas usam o mesmo
microfone, e sem fone o som de uma volta pela outra.

1. Abra `http://localhost:8080` e `http://127.0.0.1:8080`. O navegador trata os dois como sites
   diferentes, então cada um tem a própria sessão: entre com uma conta em cada.
2. Na primeira, crie um servidor; na segunda, entre pelo código de convite.
3. Nas duas, clique no canal de voz **Geral** e permita o microfone. O painel "Voz conectada"
   aparece em cima do seu nome, e os dois nomes aparecem abaixo do canal.
4. Fale: você se ouve na outra janela com um pequeno atraso, e o seu nome ganha um anel verde.
5. Teste o mute e o deaf pelos botões 🎤 e 🎧 ao lado do seu nome (ou Ctrl+Shift+M e
   Ctrl+Shift+D). Com o mute, a outra janela mostra 🔇 no seu nome e para de ouvir você. Com o
   deaf, você para de ouvir a outra janela e o seu microfone fecha. Desmutar em deaf volta tudo.
6. Clique em **Sair** numa delas, ou feche a aba: o nome some da outra na hora.

## API

| Rota | Auth | O que faz |
| --- | --- | --- |
| `GET /health` | | Status da API e do banco |
| `POST /auth/register` | | Cria a conta e abre a sessão (201) |
| `POST /auth/login` | | Abre a sessão |
| `POST /auth/refresh` | cookie | Troca o refresh token (rotação) e emite um access token novo |
| `POST /auth/logout` | cookie | Revoga a sessão e apaga o cookie (204) |
| `GET /users/@me` | Bearer | Usuário logado |
| `GET /users/@me/guilds` | Bearer | Servidores do usuário |
| `POST /guilds`, `GET/PATCH/DELETE /guilds/{id}` | Bearer | Cria, vê, renomeia e apaga um servidor (mudanças só para o dono) |
| `GET /guilds/{id}/members`, `DELETE /guilds/{id}/members/@me` | Bearer | Lista os membros e sai do servidor |
| `POST /invites/{code}` | Bearer | Entra no servidor pelo código de convite |
| `GET/POST /guilds/{id}/channels`, `PATCH/DELETE /channels/{id}` | Bearer | Canais de texto e voz (mudanças só para o dono) |
| `GET/POST /channels/{id}/messages` | Bearer | Histórico paginado (`before`, `after`, `limit`) e envio de mensagem |
| `POST /ws/ticket` | Bearer | Ticket de uso único (30 s) para abrir o WebSocket |
| `GET /ws?ticket=...` | ticket | WebSocket do chat: `SUBSCRIBE`, `MESSAGE_CREATE`, `TYPING_START`, `PRESENCE_UPDATE` e heartbeat |
| `POST /channels/{id}/voice/token` | Bearer | Token do LiveKit para entrar no canal de voz (membros, 5 min) |

O access token (JWT, 15 min) vai no corpo da resposta e é enviado como
`Authorization: Bearer`. O refresh token (30 dias) fica num cookie `HttpOnly` com `Path=/auth`.
Erros respondem `{"code": "...", "message": "..."}`, com os códigos de `proto::ErrorCode`.
Quem não é membro de um servidor recebe 404 nas rotas dele. Os detalhes estão em
`docs/architecture/06-servidores.md` e `docs/architecture/07-websocket.md`.

