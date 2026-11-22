use axum::{
    Json,
    extract::{Path, State},
};
use proto::{ChannelId, ChannelKind, VoiceTokenResponse};

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
