use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use proto::{
    Channel, ChannelId, CreateChannelRequest, GuildId, UpdateChannelRequest,
    guild::validate_channel_name,
};
use uuid::Uuid;

use crate::{
    access::{ChannelRow, channel_member, guild_member},
    auth::AuthUser,
    error::{AppError, AppResult},
    state::AppState,
};

fn clean_name(raw: &str) -> AppResult<String> {
    let name = raw.trim().to_owned();
    validate_channel_name(&name).map_err(AppError::Validation)?;
    Ok(name)
}

/// `GET /guilds/{id}/channels`: canais na ordem da barra lateral.
pub async fn list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(guild): Path<GuildId>,
) -> AppResult<Json<Vec<Channel>>> {
    guild_member(&state.db, guild, user).await?;
    let rows: Vec<ChannelRow> = sqlx::query_as(
        "SELECT id, guild_id, name, kind, position FROM channels
         WHERE guild_id = $1 ORDER BY position, id",
    )
    .bind(guild.0)
    .fetch_all(&state.db)
    .await?;
    let channels = rows.into_iter().map(Channel::try_from).collect::<AppResult<_>>()?;
    Ok(Json(channels))
}

/// `POST /guilds/{id}/channels`: cria o canal no fim da lista. Só o dono.
pub async fn create(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(guild): Path<GuildId>,
    Json(req): Json<CreateChannelRequest>,
) -> AppResult<(StatusCode, Json<Channel>)> {
    guild_member(&state.db, guild, user).await?.require_owner()?;
    let name = clean_name(&req.name)?;
    let row: ChannelRow = sqlx::query_as(
        "INSERT INTO channels (id, guild_id, name, kind, position)
         SELECT $1, $2, $3, $4, COALESCE(MAX(position) + 1, 0) FROM channels WHERE guild_id = $2
         RETURNING id, guild_id, name, kind, position",
    )
    .bind(Uuid::now_v7())
    .bind(guild.0)
    .bind(name)
    .bind(req.kind.as_str())
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(row.try_into()?)))
}

/// `PATCH /channels/{id}`: renomeia. Só o dono do servidor.
pub async fn update(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(channel): Path<ChannelId>,
    Json(req): Json<UpdateChannelRequest>,
) -> AppResult<Json<Channel>> {
    channel_member(&state.db, channel, user).await?.guild.require_owner()?;
    let name = clean_name(&req.name)?;
    let row: ChannelRow = sqlx::query_as(
        "UPDATE channels SET name = $2 WHERE id = $1
         RETURNING id, guild_id, name, kind, position",
    )
    .bind(channel.0)
    .bind(name)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(row.try_into()?))
}

/// `DELETE /channels/{id}`: apaga o canal e as mensagens dele. Só o dono do servidor.
pub async fn delete(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(channel): Path<ChannelId>,
) -> AppResult<StatusCode> {
    channel_member(&state.db, channel, user).await?.guild.require_owner()?;
    sqlx::query("DELETE FROM channels WHERE id = $1").bind(channel.0).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
