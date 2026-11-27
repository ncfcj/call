# Botão de configurações gerais no app desktop

Status: ready-for-agent

## O que fazer

- Um botão "Configurações" no app desktop (perto de "Sair" e dos botões de voz, em
  `channel_sidebar`, `desktop/src/chat.rs`) que abre um diálogo com seções.
- Seção **Áudio**: as mesmas opções do diálogo do botão "Áudio" de hoje (`audio_settings`):
  microfone, saída e Push-to-Talk. Reaproveitar o mesmo código, sem duplicar.
- Seção **Sobre**: o nome do app e a versão (`env!("CARGO_PKG_VERSION")`, a mesma que o
  atualizador usa em `desktop/src/update.rs`).
- A decidir: o botão "Áudio" continua como atalho para a seção Áudio ou sai.

## Como conferir

Abrir Configurações: a seção Áudio troca o microfone e a tecla do Push-to-Talk como o diálogo de
hoje, e a seção Sobre mostra a mesma versão do instalador (ex.: 0.1.0).
