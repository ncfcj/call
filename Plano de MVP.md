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

- [x] Monorepo num workspace Cargo, com os crates compartilhados proto, api-client e core, o servidor em `server/` e o client Leptos (Trunk) em `web/`
- [x] `docker compose` local com Postgres e `livekit-server --dev`, para não gastar cota em desenvolvimento
- [x] Migrations `sqlx` iniciais: `users`, `guilds`, `guild_members`, `channels` (kind: text | voice), `messages`
- [x] CI com `cargo clippy`, `cargo test` e build do front

Pronto quando: `GET /health` responde e o front carrega apontando para a API local.

**Situação: concluída** (commit `4b5ef9c`, na `main`).

### Fase 1: autenticação, servidores e chat de texto (1 a 2 semanas)

- [x] `POST /auth/register`, `/auth/login` e `/auth/refresh`: Argon2, JWT de acesso de 15 min, refresh rotativo em cookie `HttpOnly`. Também `POST /auth/logout` e `GET /users/@me`
- [x] CRUD mínimo de servidores (*guilds*) e canais, mais convite por código. Mudanças só para o dono; quem não é membro recebe 404
- [x] `GET /channels/:id/messages?before=<id>&limit=50` para o histórico paginado. Ganhou também `after=<id>`, usado para recuperar o que se perdeu ao reconectar
- [x] `POST /ws/ticket` gera um ticket de uso único (30 s). `GET /ws?ticket=...` faz o upgrade
- [x] Protocolo de eventos: `HELLO`, `SUBSCRIBE`, `MESSAGE_CREATE`, `TYPING_START`, `PRESENCE_UPDATE` e heartbeat a cada 30 s. Mais `SUBSCRIBED` e `ERROR`
- [x] Client: lista de servidores e canais, chat e reconexão com *backoff* exponencial que recarrega o que perdeu desde o último id

Pronto quando: dois usuários em navegadores diferentes trocam mensagens em tempo real. Um usuário fora do servidor não recebe eventos do canal. Reiniciar o backend não perde mensagens.

**Situação: concluída.** Entrou na `main` pelos PRs #1 (autenticação) e #3 (o resto da fase). Os três critérios têm testes de integração e foram conferidos no navegador.

| Etapa | Commit | Teste funcional |
| --- | --- | --- |
| Autenticação | `e05b55a` | `server/tests/auth.rs` |
| Servidores, canais, convites e histórico | `50b0e6c` | `scripts/teste-servidores.ps1` |
| WebSocket | `27eaad2` | `scripts/teste-websocket.ps1` |
| Client Leptos | `121c8e2` | `scripts/teste-client.ps1 -Convite <código>` |

### Fase 2: voz com WebRTC (1 a 2 semanas)

- [x] `POST /channels/:id/voice/token`: confere a participação no servidor e emite um token LiveKit (sala = id do canal, TTL curto). Validade de 5 min, só microfone, até 10 pessoas por sala
- [x] 2a. Ponte do crate voice (wasm-bindgen com livekit-client) e call 1:1 entrando e saindo do canal de voz. O `livekit-client` 2.25.0 fica em `web/vendor/`, sem npm
- [x] 2b. Grupo de até 6 pessoas, com indicador de quem está falando. Testado com 6 participantes; o anel acompanha quem fala em menos de 1 s
- [x] Mute, deaf e modo detecção de voz (microfone aberto com supressão de ruído). Regras do Discord, atalhos Ctrl+Shift+M e Ctrl+Shift+D; o deaf só aparece para os outros com o webhook
- [x] Seletor de microfone e fone de saída. Vale no meio da chamada e fica guardado; dispositivo desconectado cai no padrão
- [x] Endpoint de webhook do LiveKit, mais um mapa de estado de voz difundido pelo WS de chat. O client também avisa o canal, o mute e o deaf pelo WS; a saída de uma conexão antiga (outra aba) não tira ninguém da lista

Pronto quando: duas pessoas em redes diferentes (4G e Wi-Fi de outra casa) conversam sem eco perceptível. Mute e deaf aparecem na barra lateral dos outros em menos de 1 s. Fechar a aba remove o usuário do canal.

**Situação: implementada, em validação.** Branch `fase-2`, PR #4. O critério "redes diferentes (4G e Wi-Fi de outra casa)" depende do LiveKit acessível de fora da rede local, e fica para o deploy da Fase 3 (ou um teste com o LiveKit Cloud). Mute e deaf chegam à barra lateral dos outros em poucos milissegundos, e fechar a aba tira a pessoa do canal na hora.

| Etapa | Situação | Teste funcional |
| --- | --- | --- |
| Token de voz | Pronta | `scripts/teste-voz-token.ps1` |
| Ponte `voice` e chamada 1:1 | Pronta | Manual, duas janelas (README) |
| Grupo e indicador de quem fala | Pronta | Robôs no navegador (6 participantes) |
| Mute, deaf e detecção de voz | Pronta | Manual, duas janelas (README) |
| Seletor de dispositivos | Pronta | Manual (README) |
| Webhook e estado de voz no WebSocket | Pronta | Manual, duas janelas (README) |

### Fase 3: polimento e deploy (cerca de 1 semana)

- [ ] VM Oracle com Caddy (TLS automático) na frente do binário Rust, serviço `systemd` e Neon em produção
- [ ] Front no Cloudflare Pages, com CORS restrito ao domínio dele
- [ ] Rate limit em login e envio de mensagens. Tamanho máximo de mensagem
- [ ] Logs estruturados com `tracing` e um painel simples de uso das cotas (minutos do LiveKit, armazenamento do Neon)
- [ ] Retenção de mensagens de texto (TTL), para guardar o mínimo necessário. Detalhes abaixo
- [ ] Build Tauri para Windows (instalador MSI ou NSIS) com o mesmo client Leptos. Pós-MVP: Push-to-Talk com tecla configurável e atalho global via `global-shortcut`

Pronto quando: URL pública, cinco pessoas numa call de 30 min sem quedas, e o consumo do mês dentro das cotas gratuitas.

#### Retenção de mensagens (TTL)

Objetivo: o banco guarda só as mensagens recentes, e o custo de armazenamento para de crescer com o tempo de uso. Hoje toda mensagem fica para sempre. Uma mensagem curta ocupa de 150 a 300 bytes com o índice, e o Neon Free tem 1 GB.

Decisões a tomar:

- [ ] **Prazo padrão.** Sugestão: 30 dias. Quanto menor o prazo, menor o custo e menos dado para proteger (Fase 4)
- [ ] **Quem escolhe.** Um prazo global no servidor (`MESSAGE_TTL_DAYS`) ou um prazo por servidor (guild), que o dono pode reduzir mas não aumentar além do global
- [ ] **Limite por quantidade.** Opcional: manter no máximo N mensagens por canal, além do prazo

Implementação prevista:

- **Expurgo periódico:** uma tarefa no próprio `call-server` (um `tokio::time::interval`, por exemplo a cada hora) apaga em lotes as mensagens vencidas. Como o id é UUIDv7, o tempo está no próprio id: dá para apagar com `WHERE id < <UUIDv7 do instante de corte>` usando o índice que já existe.
- **Espaço de volta no disco:** no Postgres, um `DELETE` não devolve espaço na hora; o `VACUUM` só reaproveita o espaço. Se o volume crescer, particionar `messages` por mês e apagar a partição inteira (`DROP TABLE`), que devolve o espaço na hora e não gera trabalho de `VACUUM`.
- **Client:** o histórico termina no limite do prazo. A tela avisa algo como "mensagens com mais de 30 dias são apagadas", e o `has_older` do `MessageLog` fica falso ao chegar lá.
- **Reconexão:** um client que ficar offline por mais tempo que o prazo simplesmente não recupera o que venceu. O `?after=<id>` continua funcionando.
- **Medição:** o painel de cotas mostra o tamanho da tabela (`pg_total_relation_size('messages')`) e quantas mensagens o expurgo apagou.

Pronto quando: mensagens mais velhas que o prazo somem sozinhas, o tamanho de `messages` se estabiliza com o uso contínuo, e o client deixa claro até onde vai o histórico.

### Fase 4: segurança dos dados (pós-MVP, data a definir)

Hoje o texto das mensagens fica puro no Postgres, protegido só pelo TLS da conexão e pela criptografia de disco do provedor. O áudio já trafega cifrado (DTLS-SRTP do WebRTC) entre o client e o SFU, mas o SFU decifra cada pacote para encaminhar. Esta fase decide até onde ir, em dois níveis:

| Nível | Texto | Áudio | Quem consegue ler | Custo |
| --- | --- | --- | --- | --- |
| **A. Cifrado em repouso** | O servidor cifra o `content` antes de gravar (AES-256-GCM), com uma chave por servidor (guild) guardada fora do banco | Já atendido pelo DTLS-SRTP no transporte; não há gravação de áudio | O servidor, mas não quem só tiver acesso ao banco ou a um backup | Baixo: muda a coluna para `BYTEA` e o caminho de gravar e ler |
| **B. Ponta a ponta (E2EE)** | O client cifra antes de enviar; o servidor só guarda e repassa o texto cifrado. Chaves de grupo por servidor, com MLS (`openmls`) | E2EE do LiveKit (SFrame via Insertable Streams), com a chave distribuída pelo mesmo grupo MLS | Só os membros | Alto: gestão de chaves no client, entrada e saída de membros, histórico para quem entra depois |

- [ ] Decidir o nível (A, B ou A agora e B depois) e o modelo de ameaça: vazamento do banco, operador do servidor, provedor do SFU
- [ ] Texto, nível A: coluna `content` em `BYTEA` com nonce, chave por servidor derivada de uma chave mestra fora do banco (variável de ambiente ou KMS), migração das mensagens existentes e rotação de chave
- [ ] Texto, nível B: chaves no client (WebCrypto ou `openmls` em WASM), convite que entrega a chave do grupo, troca de chave quando alguém sai, e o que acontece com o histórico de quem entra depois
- [ ] Áudio, nível B: ativar o E2EE do `livekit-client` na ponte do crate `voice` e conferir o suporte no Firefox, no Chrome e no WebView2 do Tauri
- [ ] Revisar o que continua visível mesmo com E2EE: quem fala com quem e quando, nomes de servidores e canais, presença

Impactos a considerar antes de escolher:

- **Limite de 4000 caracteres:** com texto cifrado, o `CHECK` do banco passa a valer sobre o tamanho cifrado, e a validação do conteúdo fica só no client (nível B).
- **Busca e moderação:** com o nível B o servidor não lê as mensagens, então busca no servidor, filtros e denúncia com conteúdo ficam impossíveis ou precisam rodar no client.
- **Perda de chave:** no nível B, quem perder todas as sessões perde o histórico, a menos que exista um backup de chave.
- **Desempenho:** o nível A acrescenta microssegundos por mensagem. O E2EE de áudio cifra cada quadro no client e aumenta o uso de CPU em chamadas grandes.

Pronto quando: o nível escolhido está implementado, um dump do banco não revela o texto das mensagens, e (no nível B) o SFU encaminha áudio que ele mesmo não consegue decifrar.

## Ideias para o futuro (opcionais, sem data)

Conceitos anotados para revisitar depois do MVP. Nenhum está planejado nem tem implementação decidida.

### Controle remoto do computador compartilhado (estilo AnyDesk e TeamViewer)

Quem compartilha a tela pode dar a outra pessoa o controle do mouse e do teclado do seu computador, dentro de uma chamada.

- Depende de compartilhamento de tela, que está fora do MVP.
- O host precisa autorizar cada sessão de controle e poder revogar na hora.
- Exige o app desktop (Tauri) no host: o navegador não consegue injetar mouse e teclado no sistema.

### Jogo e controle remoto (estilo Parsec)

O convidado usa o próprio controle (gamepad), mouse e teclado, e o programa envia esses comandos ao host, que os aplica como se fossem um controle local. A ideia é jogar junto um jogo que roda só na máquina do host.

- Precisa de vídeo com latência muito baixa e taxa de quadros alta, bem mais exigente que a voz.
- No host, precisa de um controle virtual para o sistema reconhecer o gamepad remoto.
- Também exige o app desktop no host e a autorização explícita dele.

## Decisões tomadas na implementação

O que mudou em relação ao plano acima, e por quê. Os detalhes estão em `docs/architecture/`.

| Assunto | Plano | Implementado | Motivo |
| --- | --- | --- | --- |
| Queries do `sqlx` | Checadas em compilação | Checadas em runtime, cobertas por testes de integração com Postgres | A checagem em compilação exige o banco ligado ou um cache offline em todo build |
| Fan-out do chat | Um `broadcast` por canal de texto | Inscrição por servidor (`SUBSCRIBE {guild_id}`), com uma fila por conexão num `Hub` em memória | Quem sai do servidor para de receber na hora, e canais novos entram sem se reinscrever. Continua sendo instância única |
| Envio de mensagens | Não definido | `POST /channels/{id}/messages`; o WebSocket só difunde o `MESSAGE_CREATE` depois de gravar | Quem reconecta acha no histórico tudo o que já foi difundido |
| Presença | Por usuário | Por servidor: online enquanto houver uma conexão inscrita nele | Cada servidor mostra só os próprios membros |
| Conexão lenta | Não definido | Fila de 256 eventos; cheia, a conexão é desligada e o client reconecta | Um client travado não segura a memória do servidor |
| Servidor novo | Não definido | Já nasce com os canais `geral` (texto) e `Geral` (voz) | Dá para conversar logo depois de criar |
| Dono do servidor | Não definido | Não pode sair; para isso, apaga o servidor. Sem transferência de dono | Mantém o MVP simples |
| Rust mínimo | 1.85 | 1.88 | O Leptos 0.8.23 exige 1.88. Com 1.85, ele ficava preso na 0.8.3, que avisava de código a ser recusado pelo Rust |
| Endereço local | `127.0.0.1` | `BIND_ADDR` aceita vários endereços; o padrão escuta em `127.0.0.1` e `[::1]`, e `WEB_ORIGIN` inclui `http://[::1]:8080` | O `localhost` resolve primeiro para `::1`, e o Firefox não tenta o IPv4 depois |
| IP da mídia do LiveKit em dev | `--node-ip 127.0.0.1` | `LIVEKIT_NODE_IP` no `.env`, com o IP da máquina na rede local | O Firefox não aceita candidatos ICE de loopback e a chamada nunca conectava |
| Ponte JS da voz | Snippet do wasm-bindgen | `<script data-trunk>` com hash no nome, expondo o global `CallVoice` | A URL do snippet não mudava com o conteúdo; com a versão velha em cache, a integridade falhava e o app abria em branco |
| URL da API no client | Fixa no build | Mesmo host da página, na porta 3000 (ou `CALL_API_URL` no build) | O navegador trata `localhost` e `127.0.0.1` como sites diferentes e não manda o cookie de refresh |

Como cada etapa é entregue: um PR por fase da branch `fase-N` para a `main`, com um commit e um script de teste funcional em `scripts/` por etapa.

### Pendências conhecidas

- **Cookie no deploy (Fase 3):** o refresh usa `SameSite=Lax` e funciona com o client e a API no mesmo site. Com o Cloudflare Pages e a VM em domínios diferentes, vai precisar de `SameSite=None` ou de um subdomínio em comum.
- **Erros de extração em texto puro:** JSON malformado, UUID inválido na URL ou query inválida respondem texto do axum, e não `{code, message}`.
- **Uma sessão por navegador:** o cookie de refresh é um só. Para testar com dois usuários, use dois navegadores ou uma janela anônima.
- **Transferência de dono** e **edição ou exclusão de mensagens** ainda não existem.
- **Voz pelo endereço `http://[::1]:8080` no Firefox:** a entrada na chamada falha, e a tentativa nem chega ao LiveKit (o Chromium funciona nesse endereço). Só afeta o desenvolvimento local: use `http://localhost:8080`. Para investigar, abra o console do Firefox (F12) ao clicar no canal de voz por `[::1]`.
- **Retenção das mensagens:** hoje toda mensagem fica para sempre. O TTL está planejado na Fase 3.
- **Criptografia das mensagens de texto e de áudio:** planejada na Fase 4.

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
