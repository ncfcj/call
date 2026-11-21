# Arquitetura

Diagramas Mermaid do que **já existe no código** (Fase 0 e, da Fase 1, a autenticação, os servidores, canais e mensagens, o WebSocket e o client). Atualize-os junto com o código:
um diagrama desatualizado engana mais do que a falta dele.

| Arquivo | O que mostra |
| --- | --- |
| [01-crates.md](01-crates.md) | Crates do workspace e quem depende de quem |
| [02-tipos.md](02-tipos.md) | Structs, enums e funções, e como se ligam |
| [03-fluxos.md](03-fluxos.md) | Subida do servidor e a rota `GET /health` |
| [04-banco.md](04-banco.md) | Esquema do Postgres |
| [05-auth.md](05-auth.md) | Cadastro, login, refresh rotativo e módulos de auth do servidor |
| [06-servidores.md](06-servidores.md) | Rotas de servidores, canais, convites e histórico, e o módulo de permissão |
| [07-websocket.md](07-websocket.md) | Ticket, eventos, inscrição por servidor, presença e o hub de fan-out |
| [08-client.md](08-client.md) | Sessão no client, reconexão com backoff, histórico e as telas |

A visão geral do MVP, com o LiveKit e a hospedagem, está no `Plano de MVP.md`.
