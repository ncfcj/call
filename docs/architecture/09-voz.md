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

## Próximas etapas

- Ponte do crate `voice` com o `livekit-client` (JS) e a chamada 1:1 no client.
- Grupo com indicador de quem fala; mute, deaf e detecção de voz; seletor de dispositivos.
- Webhook do LiveKit e o estado de voz difundido pelo WebSocket do chat.
