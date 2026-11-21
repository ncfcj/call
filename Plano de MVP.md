# MVP de chat de voz e texto estilo Discord: arquitetura e plano

Nov 20, 2026 · @Nilton

## Resumo e premissas

Recomendação: um servidor Rust (axum + tokio + sqlx) cuida de autenticação, servidores/canais, chat via WebSocket e emissão de tokens de voz. A mídia de áudio fica num SFU pronto (LiveKit), e não em código próprio. O client também é em Rust (Leptos compilado para WebAssembly): roda no navegador e, empacotado no Tauri, vira o app Windows leve, com o mesmo código.

Premissas adotadas por padrão (mude se não servirem):

- **Web primeiro, Tauri na Fase 3.** O Tauri reaproveita 100% da UI web. O Push-to-Talk fica para depois do MVP e entra junto com o Tauri, que permite um atalho global.
- **SFU gerenciado no início (LiveKit Cloud, plano Build gratuito) e self-host como saída.** O mesmo SDK funciona nos dois casos, então trocar é questão de mudar a URL e as chaves.
- **Servidor Rust sempre ligado numa VM gratuita (Oracle Always Free).** PaaS gratuitos que hibernam derrubam WebSockets.
- **Grupo pequeno:** até \~10 pessoas por canal de voz, só áudio, sem vídeo e sem compartilhamento de tela no MVP.

Pergunta em aberto que muda o plano:

- Você aceita cadastrar um cartão na Oracle Cloud? A conta pede cartão, mesmo sem cobrar. Se não, o backend vai para um PaaS com hibernação e o áudio fica 100% no LiveKit Cloud.

## Arquitetura do MVP

Cinco componentes, e o áudio nunca passa pelo servidor Rust: o client fala com a API para identidade e chat, e direto com o SFU para a mídia.

&#91;embedded content: arquitetura do MVP · 5 componentes\]

Os três fluxos que o MVP precisa provar:

1. **Entrada:** o client faz login via REST. A API valida no Postgres e devolve um JWT curto mais um refresh em cookie.
2. **Chat:** o client pede um ticket e abre o WSS. Ele se inscreve nos canais do servidor. Cada mensagem é gravada no Postgres e depois difundida aos inscritos daquele canal.
3. **Voz:**
   1. O client pede um token de voz via REST. A API confere a permissão e assina o token localmente com a chave do LiveKit, sem chamar o SFU.
   2. O client conecta direto ao SFU por WebRTC.
   3. Os webhooks de entrou/saiu chegam à API, que avisa pelo WS quem está em cada canal de voz.

## Stack do client

Escolha: o client é escrito em Rust com **Leptos** (modo CSR, compilado para WebAssembly com Trunk). O mesmo app roda no navegador e, dentro do **Tauri 2**, vira o client Windows leve. O Tauri usa o WebView2, que já vem no Windows 10/11, e por isso o instalador é pequeno e o consumo de memória fica bem abaixo de um app Electron.

A voz usa o WebRTC do navegador ou do WebView, tanto na web quanto no Tauri. O client Rust chama o SDK JS `livekit-client` por uma ponte fina de `wasm-bindgen`. O SDK Rust do LiveKit não entra aqui, porque depende do libwebrtc nativo e não roda em WASM.

| Opção | Reaproveitamento web ↔ Windows | Peso no Windows | Voz | Situação |
| --- | --- | --- | --- | --- |
| **Leptos (Rust → WASM) + Tauri 2** | Praticamente total: o mesmo código nos dois | Leve: WebView2 do sistema, instalador de poucos MB | `livekit-client` via `wasm-bindgen`, igual nos dois alvos | **Escolhida** |
| Dioxus (Rust → WASM, desktop próprio) | Praticamente total, com um CLI para web e desktop | Leve, com WebView próprio em vez do Tauri | Mesma ponte JS | Alternativa equivalente. Perde os plugins do Tauri (atalho global, instalador, updater) |
| React + TS + Tauri | Total | Igual ao Leptos | SDK JS direto, sem ponte | Descartada: a UI não seria em Rust |
| UI nativa (iced, egui, Slint) + SDK Rust do LiveKit | Parcial: a UI web desenha em canvas e a voz precisa de duas implementações | O mais leve, sem WebView | libwebrtc nativo, com build pesado no Windows | Só se o consumo mínimo de memória virar diferencial |

**Crates do client no workspace Cargo** (a UI pode ser trocada sem reescrever o resto):

| Crate | Conteúdo | Alvos |
| --- | --- | --- |
| `proto` | Tipos de eventos do WebSocket e DTOs REST (`serde`). **Compartilhado com o servidor** | Servidor, WASM |
| `api-client` | Chamadas REST com `reqwest` (funciona em WASM) e o ticket do WS | WASM |
| `core` | Estado de servidores, canais, presença e voz, sem dependência de UI | WASM |
| `voice` | Ponte `wasm-bindgen` com o `livekit-client`: conectar, sair, ligar/desligar microfone, deaf, eventos de quem fala e de participantes | WASM |
| `web` | Componentes Leptos, roteamento e o build Trunk | WASM |
| `src-tauri` | Casca desktop: janela, instalador e, no pós-MVP, o atalho global de PTT | Windows nativo |

Custo dessa escolha: o ecossistema de componentes é menor que o do React e a ponte com o JS do LiveKit é escrita à mão. Para conter isso, mantenha a ponte `voice` pequena (cerca de seis funções e dois tipos de evento) e com testes no navegador.

## Stack do servidor Rust

O servidor Rust é o *control plane*: identidade, permissões, chat e estado. Ele não encaminha pacotes de áudio, que ficam a cargo do SFU. Assim, o Rust faz o que faz bem (concorrência segura, WebSockets) e você não reescreve um SFU no MVP.

| Camada | Crate | Uso no MVP |
| --- | --- | --- |
| Runtime | `tokio` | Async, tasks por conexão WS, canais internos |
| HTTP + WebSocket | `axum` (feature `ws`), `tower-http` | REST, upgrade WS, CORS, tracing, limites de corpo |
| Banco | `sqlx` (postgres, rustls, migrate) | Queries checadas em compilação, migrations versionadas |
| Auth | `argon2`, `jsonwebtoken` | Hash de senha, JWT de acesso curto + refresh |
| Voz | `livekit-api` (features `access-token`, `webhooks`) | Gera o token de entrada na sala e valida webhooks de entrou/saiu |
| Serialização | `serde`, `serde_json` | Tipos no crate proto, compartilhado com o client. Envelope de eventos WS tipado (`enum` com `#[serde(tag = "t")]`) |
| Erros e logs | `thiserror`, `anyhow`, `tracing` | Erros de domínio mapeados para HTTP e códigos WS |
| IDs | `uuid` (v7) | IDs ordenáveis por tempo, úteis para paginar mensagens |

**Fan-out do chat:** um `tokio::sync::broadcast` por canal de texto ativo, guardado num `DashMap<ChannelId, Sender>`. A mensagem é gravada no Postgres e depois difundida. Isso só funciona com uma instância. Para escalar horizontalmente, troque por Redis pub/sub ou NATS (fora do MVP).

**Abordagem de mídia (SFU), da mais rápida à mais "Rust":**

1. **LiveKit (recomendado).** SFU open source em Go, com SDKs web, Rust e mobile. Roda gerenciado (LiveKit Cloud) ou self-hosted com o mesmo protocolo. O Rust só emite tokens com `room_join` e `can_publish`.
2. **Cloudflare Realtime SFU.** Mais baixo nível: você orquestra *tracks* via API HTTP, sem conceito de sala pronto. Tem uma cota gratuita generosa de banda, mas exige mais código de sinalização seu.
3. **SFU próprio em Rust (`str0m`, `oxpulse-sfu-kit`, `webrtc-rs`).** Ótimo para aprender e para um diferencial futuro. No MVP, custa semanas em ICE, DTLS, RTCP e estimativa de banda.
4. **P2P mesh sem SFU.** Serve só para 1:1 ou 3 pessoas. O upload de cada cliente cresce com o número de participantes, e você ainda precisa de TURN.

**Estado de voz:** a fonte da verdade é o LiveKit. O Rust recebe os webhooks `participant_joined` e `participant_left`, mantém um mapa em memória (canal → membros e flags de mute/deaf) e difunde pelo WebSocket de chat. É isso que mostra "quem está na call" na barra lateral, mesmo para quem não entrou.

**Mute, deaf, PTT e detecção de voz (no client, via SDK):**

- *Mute:* `setMicrophoneEnabled(false)`.
- *Deaf:* mutar o microfone e cancelar a inscrição (ou o volume) de todas as faixas remotas.
- *PTT (pós-MVP):* `keydown`/`keyup` liga e desliga o microfone, com \~150 ms de *release delay* para não cortar o fim da fala.
- *Detecção de voz:* microfone aberto com a supressão de ruído do navegador, mais o evento de *active speaker* do LiveKit para o anel verde.
- O client avisa o servidor da mudança de estado pelo WS, para refletir na barra lateral.

## Banco e hospedagem gratuitos

Combinação recomendada: Cloudflare Pages (front) + VM Oracle Always Free (Rust API sempre ligada) + Neon Free (Postgres) + LiveKit Cloud Build (voz). Nessa configuração nada hiberna no caminho do WebSocket. O banco hiberna, mas acorda na primeira query.

| Serviço | Papel | Limites reais do plano gratuito | Impacto no app |
| --- | --- | --- | --- |
| [Neon Free](https://neon.com/faqs/free-plan-limits-and-quotas) | Postgres | 100 CU-hours/mês e 1 GB por projeto, 5 GB de transferência. Desliga após 5 min ocioso | A primeira query depois de ociosidade é mais lenta. Acima de 1 GB, as escritas falham |
| [Supabase Free](https://supabase.com/pricing) | Postgres (alternativa) | 500 MB, 5 GB de banda, 2 projetos. Pausa após 1 semana sem atividade | Reativação manual pelo painel. Ruim para um PoC que fica dias parado |
| [Oracle Always Free](https://docs.oracle.com/en-us/iaas/Content/FreeTier/resourceref.htm) | VM do backend (e SFU/TURN se self-host) | Ampere A1 Arm: 4 OCPU e 24 GB no total, até 4 VMs. Pede cartão no cadastro | Sempre ligada, com IP público e UDP livre. A capacidade A1 varia por região. Compile para `aarch64` |
| [Render Free](https://render.com/docs/free) | Backend (alternativa sem VM) | Hiberna após 15 min sem HTTP nem mensagem WS. Leva cerca de 1 min para voltar. Postgres grátis expira em 30 dias | WebSockets caem quando hiberna. Sem UDP, então não serve para mídia |
| [Koyeb Free](https://www.koyeb.com/docs/reference/instances) | Backend (alternativa) | 1 instância, 512 MB, 0,1 vCPU. Escala a zero após 1 h sem tráfego | CPU mínima, mas suficiente para um PoC de chat. Cold start ao acordar |
| [LiveKit Cloud Build](https://livekit.com/blog/towards-a-future-aligned-pricing-model) | SFU + TURN gerenciado | US$ 0: 100 participantes simultâneos, 50 GB de banda, 5.000 minutos de conexão/mês. [Cota é teto rígido](https://docs.livekit.io/cloud/quotas-and-limits) | Estourou, novas conexões falham até o mês virar. Sem cobrança surpresa |
| [Cloudflare Realtime](https://developers.cloudflare.com/realtime/sfu/platform/pricing/) | SFU/TURN (alternativa) e [STUN grátis e ilimitado](https://developers.cloudflare.com/realtime/turn/faq/) | 1.000 GB/mês grátis, compartilhados entre SFU e TURN | Serve como TURN para um LiveKit self-hosted, ou como SFU de baixo nível |
| Cloudflare Pages | Front estático (SPA) | Gratuito para sites estáticos | Sem servidor: o front só fala com a API e o SFU |

**Conta de padeiro para o LiveKit Build:** o limite que pega primeiro são os 5.000 minutos de conexão, não a banda. Cinco pessoas numa call de 1 h somam 300 minutos, ou seja, cerca de 16 sessões assim por mês. A banda sobra: Opus de voz usa dezenas de kbps por faixa.

**Saída quando a cota apertar:** suba o servidor LiveKit open source na mesma VM Oracle (Docker, com TURN embutido). Para isso, abra as portas UDP do range de mídia na *security list* da Oracle e no firewall do Linux. O código do client e do Rust não muda.

**Alternativa sem cartão:** Pages + Koyeb ou Render + Neon + LiveKit Cloud. Funciona para demos agendadas. Pingar o serviço para mantê-lo acordado contraria a intenção do plano gratuito, segundo o próprio Render, então trate a hibernação como dada e faça o client reconectar sozinho.

## Plano de execução em fases

Quatro fases curtas, cada uma com um critério de aceite testável em dois navegadores. As durações são estimativas para uma pessoa em meio período.

### Fase 0: fundação (1 a 2 dias)

- [ ] Monorepo num workspace Cargo, com os crates compartilhados proto, api-client e core, o servidor em `server/` e o client Leptos (Trunk) em `web/`
- [ ] `docker compose` local com Postgres e `livekit-server --dev`, para não gastar cota em desenvolvimento
- [ ] Migrations `sqlx` iniciais: `users`, `guilds`, `guild_members`, `channels` (kind: text | voice), `messages`
- [ ] CI com `cargo clippy`, `cargo test` e build do front

Pronto quando: `GET /health` responde e o front carrega apontando para a API local.

### Fase 1: autenticação, servidores e chat de texto (1 a 2 semanas)

- [ ] `POST /auth/register`, `/auth/login` e `/auth/refresh`: Argon2, JWT de acesso de 15 min, refresh rotativo em cookie `HttpOnly`
- [ ] CRUD mínimo de servidores (*guilds*) e canais, mais convite por código
- [ ] `GET /channels/:id/messages?before=<id>&limit=50` para o histórico paginado
- [ ] `POST /ws/ticket` gera um ticket de uso único (30 s). `GET /ws?ticket=...` faz o upgrade
- [ ] Protocolo de eventos: `HELLO`, `SUBSCRIBE`, `MESSAGE_CREATE`, `TYPING_START`, `PRESENCE_UPDATE` e heartbeat a cada 30 s
- [ ] Client: lista de servidores e canais, chat e reconexão com *backoff* exponencial que recarrega o que perdeu desde o último id

Pronto quando: dois usuários em navegadores diferentes trocam mensagens em tempo real. Um usuário fora do servidor não recebe eventos do canal. Reiniciar o backend não perde mensagens.

### Fase 2: voz com WebRTC (1 a 2 semanas)

- [ ] `POST /channels/:id/voice/token`: confere a participação no servidor e emite um token LiveKit (sala = id do canal, TTL curto)
- [ ] 2a. Ponte do crate voice (wasm-bindgen com livekit-client) e call 1:1 entrando e saindo do canal de voz
- [ ] 2b. Grupo de até 6 pessoas, com indicador de quem está falando
- [ ] Mute, deaf e modo detecção de voz (microfone aberto com supressão de ruído)
- [ ] Seletor de microfone e fone de saída
- [ ] Endpoint de webhook do LiveKit, mais um mapa de estado de voz difundido pelo WS de chat

Pronto quando: duas pessoas em redes diferentes (4G e Wi-Fi de outra casa) conversam sem eco perceptível. Mute e deaf aparecem na barra lateral dos outros em menos de 1 s. Fechar a aba remove o usuário do canal.

### Fase 3: polimento e deploy (cerca de 1 semana)

- [ ] VM Oracle com Caddy (TLS automático) na frente do binário Rust, serviço `systemd` e Neon em produção
- [ ] Front no Cloudflare Pages, com CORS restrito ao domínio dele
- [ ] Rate limit em login e envio de mensagens. Tamanho máximo de mensagem
- [ ] Logs estruturados com `tracing` e um painel simples de uso das cotas (minutos do LiveKit, armazenamento do Neon)
- [ ] Build Tauri para Windows (instalador MSI ou NSIS) com o mesmo client Leptos. Pós-MVP: Push-to-Talk com tecla configurável e atalho global via `global-shortcut`

Pronto quando: URL pública, cinco pessoas numa call de 30 min sem quedas, e o consumo do mês dentro das cotas gratuitas.

## Riscos técnicos e mitigação

O maior risco é a conectividade, não o código: áudio que funciona no Wi-Fi de casa e falha em 4G ou em rede corporativa. Teste em redes diferentes desde o primeiro dia da Fase 2.

| Risco | Como aparece | Mitigação simples no MVP |
| --- | --- | --- |
| NAT simétrico ou UDP bloqueado | Conecta na sala, mas ninguém se ouve | TURN sobre TLS na porta 443. O LiveKit Cloud já inclui. No self-host, ative o TURN embutido ou use o TURN da Cloudflare. Teste sempre em 4G |
| Latência e qualidade de áudio | Atraso, robotização, cortes | SFU numa região perto dos usuários. Opus com DTX e o processamento nativo do navegador ligado (eco, ruído, ganho). Nada de reprocessar áudio no servidor |
| Eco e microfonia | Você se ouve de volta | Mantenha o cancelamento de eco do navegador. Recomende fone. Não desligue `echoCancellation` |
| Estado de voz dessincronizado | "Fantasmas" no canal depois de a aba fechar ou a rede cair | LiveKit como fonte da verdade via webhooks. Ao reconectar, reconcilie com a listagem de participantes da sala pela API |
| Auth em WebSocket | O navegador não envia o header `Authorization` no WS, e um token na URL vaza para os logs | Ticket de uso único e curto via `POST /ws/ticket`, ou autenticação na primeira mensagem. Valide o header `Origin`: CORS não protege WebSocket |
| Conexões WS caídas | Mensagens perdidas após hibernação, deploy ou troca de rede | Heartbeat, reconexão com *backoff* e recarga do histórico a partir do último id recebido |
| Políticas do navegador | Sem som até o usuário clicar. O microfone não abre fora de HTTPS | Botão explícito "Entrar na call" que destrava o áudio. HTTPS em tudo (localhost é exceção) |
| Cotas gratuitas | Voz para de conectar no fim do mês. Escritas falham no banco | Acompanhe os minutos do LiveKit e o tamanho do Neon. Use LiveKit local em dev. Tenha o self-host na VM como plano B |
| VM Oracle | Sem capacidade A1 na região. Binário x86 não roda | Escolha a região home com cuidado, porque ela não muda depois. Compile para `aarch64` na própria VM ou num runner Arm do CI. Confira a política da Oracle para instâncias ociosas |
| Instância única | O `broadcast` em memória não escala para 2 servidores | Aceitável no MVP. Isole o fan-out atrás de um trait para trocar por Redis ou NATS depois |
| Tauri no Linux | WebRTC instável no WebKitGTK | Desktop Linux usa a versão web até o PoC validar. Priorize Windows e macOS no Tauri |
| Ponte WASM com o livekit-client | Erros de tipo entre Rust e JS, eventos de voz perdidos, build WASM lento | Ponte mínima no crate voice, tipada dos dois lados e testada no navegador. Prove a call 1:1 por essa ponte antes de construir a UI de voz |
