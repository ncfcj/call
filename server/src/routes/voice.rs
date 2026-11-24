use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
};
use proto::{ChannelId, ChannelKind, GuildId, ServerEvent, UserId, VoiceState, VoiceTokenResponse};
use uuid::Uuid;

use super::users::find_user;
use crate::{
    access::channel_member,
    auth::AuthUser,
    error::{AppError, AppResult},
    state::AppState,
    voice::VOICE_TOKEN_TTL,
};

/// `POST /channels/{id}/voice/token`: confere que o usuário é membro do servidor e emite um
/// token do LiveKit para a sala do canal. O token é assinado aqui, sem chamar o LiveKit.
pub async fn token(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(channel): Path<ChannelId>,
) -> AppResult<Json<VoiceTokenResponse>> {
    state.limits.voice_token.check(user)?;
    let access = channel_member(&state.db, channel, user).await?;
    if access.channel.kind != ChannelKind::Voice {
        return Err(AppError::Validation("só canais de voz têm chamada"));
    }
    let me = find_user(&state.db, user).await?.ok_or(AppError::Unauthorized)?;
    let token = state.livekit.join_token(&me, channel)?;
    Ok(Json(VoiceTokenResponse {
        url: state.livekit.url.clone(),
        token,
        expires_in: VOICE_TOKEN_TTL.as_secs(),
    }))
}

/// `POST /livekit/webhook`: eventos do LiveKit. Sem a assinatura certa, 401.
///
/// Usa `participant_joined`, `participant_left`, `participant_connection_aborted` e
/// `room_finished` para manter o mapa de voz e avisar os membros pelo WebSocket. Responde 200
/// também para eventos que ignora, senão o LiveKit tenta de novo.
pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> AppResult<StatusCode> {
    let auth = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or("");
    let event = state.livekit.receive_webhook(&body, auth).ok_or(AppError::Unauthorized)?;

    // A sala tem o id do canal de voz, e a identidade é o id do usuário.
    let channel = event.room.as_ref().and_then(|r| Uuid::parse_str(&r.name).ok()).map(ChannelId);
    let participant = event.participant.as_ref();
    let user = participant.and_then(|p| Uuid::parse_str(&p.identity).ok()).map(UserId);
    let sid = participant.map(|p| p.sid.clone()).unwrap_or_default();
    let Some(channel) = channel else { return Ok(StatusCode::OK) };

    let events = match (event.event.as_str(), user) {
        ("participant_joined", Some(user)) => match voice_channel_guild(&state, channel).await? {
            Some(guild) => state.voice_states.join(guild, channel, user, Some(sid)),
            None => Vec::new(),
        },
        ("participant_left" | "participant_connection_aborted", Some(user)) => {
            state.voice_states.leave(channel, user, &sid).into_iter().collect()
        }
        ("room_finished", _) => state.voice_states.clear_channel(channel),
        _ => Vec::new(),
    };
    tracing::debug!(event = %event.event, %channel, "webhook do LiveKit");
    publish_voice(&state, events);
    Ok(StatusCode::OK)
}

/// Servidor do canal, se ele existir e for de voz.
async fn voice_channel_guild(state: &AppState, channel: ChannelId) -> AppResult<Option<GuildId>> {
    let guild: Option<Uuid> =
        sqlx::query_scalar("SELECT guild_id FROM channels WHERE id = $1 AND kind = 'voice'")
            .bind(channel.0)
            .fetch_optional(&state.db)
            .await?;
    Ok(guild.map(GuildId))
}

/// Avisa cada servidor das mudanças na voz dele.
pub fn publish_voice(state: &AppState, events: impl IntoIterator<Item = VoiceState>) {
    for update in events {
        state.hub.publish(update.guild_id, &ServerEvent::VoiceStateUpdate(update), None);
    }
}
