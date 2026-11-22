# Voz

Fase 2, em andamento. O áudio nunca passa pelo call-server: ele só confere a permissão e assina
um token, e o client conecta direto ao LiveKit (SFU) por WebRTC.

## Token de entrada

```mermaid
sequenceDiagram
    participant C as Client
    participant A as call-server
    participant DB as Postgres
    participant LK as LiveKit
    C->>A: POST /channels/{id}/voice/token (Bearer)
    A->>DB: é membro do servidor do canal? (access::channel_member)
    A->>A: canal é de voz? assina o JWT com LIVEKIT_API_SECRET
    A-->>C: {url, token, expires_in: 300}
    C->>LK: conecta em url com o token (WebRTC)
    LK->>LK: confere a assinatura; cria a sala se for o primeiro
```

| Campo do token | Valor | Por quê |
| --- | --- | --- |
| `sub` (identidade) | id do usuário | O LiveKit identifica o participante, e os webhooks dizem quem entrou |
| `name` | nome de exibição | Para a UI mostrar quem está na sala |
| `video.room` | id do canal de voz | Uma sala do LiveKit por canal |
| `room_join`, `can_subscribe` | sim | Entrar e ouvir os outros |
| `can_publish` com `can_publish_sources = ["microphone"]` | só o microfone | O MVP é só áudio: sem câmera nem tela |
| `can_publish_data`, `can_update_own_metadata` | não | Nada além do áudio passa pelo LiveKit |
| `room_config.max_participants` | 10 | Vale quando esta entrada cria a sala |
| validade | 5 min | Só precisa durar até conectar; depois o LiveKit renova sozinho |

Recusas: quem não é membro recebe **404**; canal de texto recebe **422**; sem login, **401**.

O token é assinado localmente com a crate `livekit-api` (features `access-token` e `webhooks`,
sem o cliente HTTP dela). Configuração: `LIVEKIT_URL` (a URL que os clients usam),
`LIVEKIT_API_KEY` e `LIVEKIT_API_SECRET`. Em desenvolvimento, o `livekit-server --dev` do
`docker compose` usa `devkey` e `secret`.

**Endereço da mídia:** o LiveKit anuncia para o WebRTC o IP de `--node-ip`, lido de
`LIVEKIT_NODE_IP` no `.env`. Em desenvolvimento, use o IP da máquina na rede local. O Firefox
não aceita candidatos ICE de loopback: com `127.0.0.1`, a sinalização conecta, mas a mídia fica
em `checking` até falhar. Em produção (Fase 3), é o IP público da VM ou o LiveKit Cloud.

## Ponte com o `livekit-client`

O SDK Rust do LiveKit depende do libwebrtc nativo e não compila para WASM. A voz usa o WebRTC
do navegador (ou do WebView2 no Tauri) pelo SDK JavaScript, numa ponte em três camadas:

| Camada | Arquivo | O que faz |
| --- | --- | --- |
| SDK | `web/vendor/livekit-client-2.25.0.umd.js` | O `livekit-client` oficial (global `LivekitClient`), guardado no repositório e conferido pelo hash do npm (`web/vendor/README.md`) |
| Ponte JS | `crates/voice/js/voice.js` | Classe `VoiceSession`: o único código que conhece a API do LiveKit. Emite eventos em JSON `{"t", "d"}` |
| Ponte Rust | `crates/voice/src` | `VoiceRoom` (`connect`, `disconnect`, `set_microphone_enabled`, `start_audio`) e `VoiceEvent`, tipados |

```mermaid
classDiagram
    direction LR
    class VoiceRoom {
        <<voice, wasm32>>
        +new(on_event) VoiceRoom
        +connect(url, token)
        +disconnect()
        +set_microphone_enabled(bool)
        +start_audio()
    }
    class VoiceEvent {
        <<enumeration>>
        Participants(Vec~VoiceParticipant~)
        ActiveSpeakers(Vec~identity~)
        Reconnecting
        Reconnected
        Disconnected(reason)
        MicError(message)
        AudioBlocked(bool)
    }
    class VoiceSession {
        <<voice.js>>
        +connect() disconnect()
        +setMicrophoneEnabled() startAudio()
    }
    class Store {
        <<web>>
        +voice: Option~VoiceState~
        +join_voice(guild, channel)
        +leave_voice()
        +unlock_audio()
    }
    VoiceRoom ..> VoiceSession : wasm-bindgen
    VoiceSession ..> VoiceEvent : JSON
    Store ..> VoiceRoom
    Store ..> VoiceEvent : on_voice_event
```

## Chamada no client

- **Entrar:** clicar num canal de voz pede o token, conecta e abre o microfone. Se já estiver
  noutro canal, sai dele antes. Uma chamada por vez.
- **Microfone negado ou ausente:** a pessoa continua na sala só ouvindo, com um aviso.
- **Áudio dos outros:** cada faixa remota toca num `<audio>` escondido, marcado com a
  identidade do participante e removido quando a faixa ou a pessoa sai.
- **Bloqueio de som do navegador:** se a reprodução automática for bloqueada, aparece o botão
  "Ativar som".
- **Sala:** abaixo do canal aparecem quem está nele, com o anel verde de quem está falando
  (`ActiveSpeakers`) e o ícone de microfone fechado.
- **Sair:** o botão "Sair" do painel de voz, sair da conta, sair do servidor ou fechar a aba.
  Ao fechar a aba ou navegar para outra página, o SDK avisa o LiveKit no `pagehide` e a saída é
  imediata. Se a página morrer sem esse evento (processo encerrado, queda de rede), o LiveKit só
  percebe pela conexão, em uns 20 s.
- **Sessão de voz numerada:** cada entrada incrementa um número; eventos de uma sala antiga que
  ainda está fechando são ignorados.

Por enquanto, quem está numa sala só é visível para quem também está nela. O mapa de voz para
todos os membros chega com o webhook do LiveKit.

## Próximas etapas

- Grupo com até 6 pessoas e o teste em redes diferentes.
- Mute, deaf e detecção de voz; seletor de dispositivos.
- Webhook do LiveKit e o estado de voz difundido pelo WebSocket do chat.
