# Chiado no microfone do app desktop

Status: ready-for-human

## O problema

Na chamada do teste da 4f, quem estava na web ouviu um chiado no áudio de quem usava o app
desktop com o Push-to-Talk ligado.

## O que o log mostra

- Os erros 10051/11001 (STUN/TURN) vêm do adaptador virtual do Hyper-V (WSL/Docker) e do Teredo,
  que não têm internet. O WebRTC tenta todas as placas e fica com a que funciona: é ruído.
- "BUNDLE group contains a codec collision": o SDK força Opus estéreo no que publica e a web
  publica com DTX. O áudio passa mesmo assim.
- "Attempt to set Windows AEC with recording already initialized" aparece poucas vezes por
  chamada, provavelmente a cada mute ou aperto do PTT. O AEC do Windows não muda; o AEC, o AGC e a
  supressão de ruído do WebRTC continuam ligados (o SDK cria a faixa do microfone com os três em
  `true`, em `create_device_audio_track`, e não deixa mudar).
- Nada no log prova a causa do chiado.

## Respostas (2026-11-27)

- O chiado é contínuo enquanto a tecla do PTT está apertada.
- Não aparece com o PTT desligado nem com o microfone sempre aberto.
- Web fora por enquanto.

## Hipótese

O áudio do WebRTC no Windows confere se alguma tecla está apertada (`GetAsyncKeyState`, das
teclas de Espaço até antes do Num Lock, o que inclui letras, números e F1 a F24) e passa isso para
o processamento de áudio, que trata o som como digitação e o abafa. Com a tecla segurada, isso
vale o tempo todo. A tecla do teste era a F8.

Para confirmar: (a) sem PTT, falar segurando uma letra num outro programa: deve chiar; (b) PTT
numa tecla fora dessa faixa (Pause, Scroll Lock ou a tecla à esquerda do 1): não deve chiar.
Se confirmar: avisar no diálogo e sugerir essas teclas, ou recusar as da faixa.

## Caminhos, conforme a resposta

- Só no começo do aperto: a supressão de ruído ou o AGC recomeçam a cada aperto. Testar trocar
  `mute()`/`unmute()` por `disable()`/`enable()` da faixa no PTT.
- Contínuo, só no desktop: o AGC do WebRTC sobe o ruído de fundo. O SDK 0.12 não deixa desligar o
  AGC de software na faixa do dispositivo; ver uma versão mais nova ou capturar por
  `NativeAudioSource` com as opções próprias.
- Também na web: é o microfone ou o nível de entrada do Windows.

## Resultado (2026-11-27)

A hipótese da tecla caiu: segurar uma letra com o microfone aberto não chia, e o PTT numa tecla
fora da faixa chia igual. Suspeita agora: o `mute()`/`unmute()` da faixa (o `set_enabled` do
WebRTC), que o PTT faz a cada aperto. Próximo teste: sem PTT, clicar em "Mudo" e voltar para
"Microfone"; se chiar depois disso, é o mute, e o bug vale também para o botão da 4c.

Confirmado: sem PTT, mutar e desmutar pelo botão também deixa chiando. Correção: o microfone abre
e fecha parando e retomando a captura (`PlatformAudio::stop_recording`/`start_recording`), sem
mutar a faixa depois de publicada. Falta o teste numa chamada real.
