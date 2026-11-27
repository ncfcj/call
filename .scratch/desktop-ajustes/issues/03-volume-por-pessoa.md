# Volume de cada pessoa na chamada de voz, de 0% a 200%

Status: needs-triage

## O problema

Numa chamada, não dá para ajustar o volume de uma pessoa só: quem fala baixo (ou alto demais)
fica assim para todos. Hoje só existe o "Sem som", que corta todo mundo.

## O que fazer

- Na lista do canal de voz (`desktop/src/chat.rs`, `voice_rows`), cada pessoa da chamada ganha
  um controle de volume, de 0% (sem som) a 200%, com 100% como padrão.
- O ajuste vale só para quem ajustou (é local, não muda o que os outros ouvem).
- Caminho provável: o volume da faixa remota no SDK do LiveKit (`RemoteAudioTrack`; conferir se a
  0.12 expõe volume/ganho no `rtc` ou se é preciso aplicar o ganho num `AudioStream` próprio).
  Acima de 100% precisa de ganho de verdade, com limite para não distorcer.
- Guardar o volume por usuário (id), para valer na próxima chamada.
- A web está congelada: só o desktop.

## Como conferir

Numa chamada com duas pessoas, pôr uma em 0%: para de ouvir só ela. Em 200%: fica mais alta que
as outras. Sair e entrar de novo: o volume continua.
