# Tab não passa do usuário para a senha no login do app desktop

Status: ready-for-agent

## O problema

Na tela de login (e de cadastro) do app desktop (`desktop/src/main.rs`, `auth_view`), apertar Tab
no campo de usuário não leva o foco para a senha. O iced não move o foco entre `text_input`
sozinho.

## O que fazer

- Tab vai para o próximo campo; Shift+Tab, para o anterior (usuário, nome, senha, convite, na
  ordem da tela).
- Caminho provável: escutar Tab com `iced::keyboard::listen`/`on_key_press` numa subscription da
  tela de login e responder com `iced::widget::operation::focus_next()` / `focus_previous()`.
- Enter na senha continua enviando o formulário.

## Como conferir

Abrir o app, digitar o usuário, Tab, digitar a senha, Enter: entra.
