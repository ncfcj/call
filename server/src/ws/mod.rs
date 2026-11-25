//! WebSocket do chat: `POST /ws/ticket` e `GET /ws?ticket=...`.
//!
//! Cada conexão roda um loop que espera, ao mesmo tempo, mensagens do client, eventos da fila
//! do [`hub::Hub`] e o prazo do heartbeat.

pub mod hub;
pub mod ticket;

use std::time::Duration;

use axum::{
    Json,
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message as WsMessage, WebSocket},
    },
    http::{HeaderMap, header},
    response::Response,
};
use proto::{ClientEvent, ErrorCode, ServerEvent, UserId, WsQuery, WsTicketResponse};
use tokio::time::Instant;

use crate::voice_state::VoiceFlags;
use crate::{
    access::{channel_member, guild_member},
    auth::AuthUser,
    error::{AppError, AppResult},
    routes::MAX_BODY,
    state::AppState,
};
use hub::ConnId;

const HEARTBEAT_INTERVAL_MS: u64 = 30_000;
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(HEARTBEAT_INTERVAL_MS);
/// Sem nenhuma mensagem do client nesse prazo, a conexão é considerada morta.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(60);

/// `POST /ws/ticket`
pub async fn ticket(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> AppResult<Json<WsTicketResponse>> {
    state.limits.ws_ticket.check(user)?;
    Ok(Json(WsTicketResponse {
        ticket: state.tickets.issue(user),
        expires_in: ticket::TICKET_TTL.as_secs(),
    }))
}

/// `GET /ws?ticket=...`: faz o upgrade.
///
/// CORS não protege WebSocket, então o `Origin` é conferido aqui. Um client fora do navegador
/// não manda `Origin` e passa só pelo ticket.
pub async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> AppResult<Response> {
    if let Some(origin) = headers.get(header::ORIGIN)
        && !state.web_origins.contains(origin)
    {
        return Err(AppError::Forbidden("origem não permitida"));
    }
    let user = state.tickets.redeem(&q.ticket).ok_or(AppError::Unauthorized)?;
    // O padrão do axum aceita mensagens de 64 MB.
    let ws = ws.max_message_size(MAX_BODY).max_frame_size(MAX_BODY);
    Ok(ws.on_upgrade(move |socket| run(state, user, socket)))
}

async fn run(state: AppState, user: UserId, mut socket: WebSocket) {
    let (conn, mut queue) = state.hub.register(user);
    tracing::info!(conn, %user, "WS conectado");

    let hello = ServerEvent::Hello {
        heartbeat_interval_ms: HEARTBEAT_INTERVAL_MS,
        user_id: user,
        message_ttl_days: Some(state.message_ttl_days),
    };
    let mut deadline = Instant::now() + HEARTBEAT_TIMEOUT;

    if send(&mut socket, &hello).await {
        loop {
            tokio::select! {
                incoming = socket.recv() => {
                    let text = match incoming {
                        Some(Ok(WsMessage::Text(text))) => text,
                        Some(Ok(WsMessage::Binary(_))) => {
                            let err = error_event(ErrorCode::Validation, "o servidor só aceita texto");
                            if !send(&mut socket, &err).await { break }
                            continue;
                        }
                        Some(Ok(WsMessage::Ping(_) | WsMessage::Pong(_))) => continue,
                        Some(Ok(WsMessage::Close(_)) | Err(_)) | None => break,
                    };
                    deadline = Instant::now() + HEARTBEAT_TIMEOUT;
                    if let Some(reply) = handle(&state, conn, user, &text).await
                        && !send(&mut socket, &reply).await
                    {
                        break;
                    }
                }
                outgoing = queue.recv() => match outgoing {
                    Some(json) => {
                        if socket.send(WsMessage::Text(json.as_ref().into())).await.is_err() { break }
                    }
                    // O hub desligou esta conexão por estar lenta demais.
                    None => break,
                },
                () = tokio::time::sleep_until(deadline) => {
                    tracing::debug!(conn, "WS sem heartbeat; fechando");
                    break;
                }
            }
        }
    }

    state.hub.unregister(conn);
    let _ = socket.send(WsMessage::Close(None)).await;
    tracing::info!(conn, %user, "WS desconectado");
}

/// Trata um evento do client. Devolve a resposta direta, se houver.
async fn handle(state: &AppState, conn: ConnId, user: UserId, text: &str) -> Option<ServerEvent> {
    let Ok(event) = serde_json::from_str::<ClientEvent>(text) else {
        return Some(error_event(ErrorCode::Validation, "evento desconhecido ou malformado"));
    };
    let result: AppResult<Option<ServerEvent>> = async {
        match event {
            ClientEvent::Heartbeat => Ok(Some(ServerEvent::HeartbeatAck)),
            ClientEvent::Subscribe { guild_id } => {
                guild_member(&state.db, guild_id, user).await?;
                let online = state.hub.subscribe(conn, guild_id);
                let voice = state.voice_states.guild(guild_id);
                Ok(Some(ServerEvent::Subscribed { guild_id, online, voice }))
            }
            ClientEvent::TypingStart { channel_id } => {
                let access = channel_member(&state.db, channel_id, user).await?;
                if access.channel.kind != proto::ChannelKind::Text {
                    return Err(AppError::Validation("só se digita em canais de texto"));
                }
                let guild_id = access.guild.guild_id;
                let typing = ServerEvent::TypingStart { guild_id, channel_id, user_id: user };
                state.hub.publish(guild_id, &typing, Some(conn));
                Ok(None)
            }
            ClientEvent::VoiceStateUpdate { channel_id, muted, deafened } => {
                let voice = &state.voice_states;
                let mut events: Vec<_> =
                    voice.set_flags(user, VoiceFlags { muted, deafened }).into_iter().collect();
                match channel_id {
                    Some(channel_id) => {
                        let access = channel_member(&state.db, channel_id, user).await?;
                        if access.channel.kind != proto::ChannelKind::Voice {
                            return Err(AppError::Validation("só canais de voz têm chamada"));
                        }
                        events.extend(voice.join(access.guild.guild_id, channel_id, user, None));
                    }
                    None => events.extend(voice.leave_declared(user)),
                }
                crate::routes::voice::publish_voice(state, events);
                Ok(None)
            }
        }
    }
    .await;
    result.unwrap_or_else(|err| Some(app_error_event(&err)))
}

fn error_event(code: ErrorCode, message: &str) -> ServerEvent {
    ServerEvent::Error { code, message: message.to_owned() }
}

fn app_error_event(err: &AppError) -> ServerEvent {
    let code = err.code();
    let message = match err {
        AppError::Internal(inner) => {
            tracing::error!(error = ?inner, "erro interno no WS");
            "erro interno do servidor".to_owned()
        }
        other => other.to_string(),
    };
    ServerEvent::Error { code, message }
}

/// Envia um evento. Devolve `false` se a conexão caiu.
async fn send(socket: &mut WebSocket, event: &ServerEvent) -> bool {
    match serde_json::to_string(event) {
        Ok(json) => socket.send(WsMessage::Text(json.into())).await.is_ok(),
        Err(err) => {
            tracing::error!(?err, "evento WS não serializou");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_errors_become_error_events_without_internal_detail() {
        let ServerEvent::Error { code, message } = app_error_event(&AppError::NotFound) else {
            panic!("esperava ERROR");
        };
        assert_eq!((code, message.as_str()), (ErrorCode::NotFound, "não encontrado"));

        let internal = AppError::Internal(anyhow::anyhow!("detalhe secreto"));
        let ServerEvent::Error { code, message } = app_error_event(&internal) else {
            panic!("esperava ERROR");
        };
        assert_eq!(code, ErrorCode::Internal);
        assert!(!message.contains("secreto"));
    }

    #[test]
    fn heartbeat_timeout_is_two_intervals() {
        assert_eq!(HEARTBEAT_TIMEOUT, HEARTBEAT_INTERVAL * 2);
        let ServerEvent::Error { code, message } = error_event(ErrorCode::Validation, "x") else {
            panic!("esperava ERROR");
        };
        assert_eq!((code, message.as_str()), (ErrorCode::Validation, "x"));
    }
}
