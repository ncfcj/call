# Não dá para selecionar nem copiar o texto das mensagens no app desktop

Status: needs-triage

## O problema

No canal de texto do app desktop (`desktop/src/chat.rs`, `message_pane`), o texto das mensagens não
pode ser selecionado com o mouse nem copiado com Ctrl+C. O widget `text` do iced só desenha o texto;
não tem seleção.

## O que fazer

O mínimo esperado: selecionar um trecho de uma mensagem e copiar (Ctrl+C ou menu). Opções a avaliar:

- Ver se a versão do iced em uso já tem texto selecionável (ou um `text_editor` só de leitura que
  permita seleção sem editar) e usar nas mensagens.
- Se não houver: copiar a mensagem inteira por um botão ou menu de contexto (clique direito) em
  cada mensagem, com `iced::clipboard::write`, como o "copiar" do convite.
- Conferir o custo: um editor por mensagem pode pesar na memória e na rolagem de canais longos
  (meta da etapa 4e).

## Como conferir

Abrir um canal com mensagens, selecionar parte de uma, Ctrl+C e colar em outro programa: sai o
trecho selecionado.
