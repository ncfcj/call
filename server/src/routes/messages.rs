use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use proto::{
    ChannelId, ChannelKind, CreateMessageRequest, Message, MessageId, MessagesQuery, User, UserId,
    message::{MESSAGES_PAGE_DEFAULT, MESSAGES_PAGE_MAX, validate_message_content},
};
use uuid::Uuid;

use crate::{
    access::{ChannelAccess, channel_member},
    auth::AuthUser,
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: Uuid,
    channel_id: Uuid,
    content: String,
    created_at: DateTime<Utc>,
    edited_at: Option<DateTime<Utc>>,
    // LEFT JOIN users: tudo nulo quando o autor foi apagado.
    author_id: Option<Uuid>,
    username: Option<String>,
    display_name: Option<String>,
}

impl From<MessageRow> for Message {
    fn from(row: MessageRow) -> Self {
        let author = match (row.author_id, row.username, row.display_name) {
            (Some(id), Some(username), Some(display_name)) => {
                Some(User { id: UserId(id), username, display_name })
            }
            _ => None,
        };
        Self {
            id: MessageId(row.id),
            channel_id: ChannelId(row.channel_id),
            author,
            content: row.content,
            created_at: row.created_at,
            edited_at: row.edited_at,
        }
    }
}

const SELECT_MESSAGE: &str = "SELECT m.id, m.channel_id, m.content, m.created_at, m.edited_at,
        u.id AS author_id, u.username, u.display_name
    FROM messages m LEFT JOIN users u ON u.id = m.author_id";

/// Canal de texto em que o usuário é membro.
async fn text_channel(
    state: &AppState,
    channel: ChannelId,
    user: UserId,
) -> AppResult<ChannelAccess> {
    let access = channel_member(&state.db, channel, user).await?;
    if access.channel.kind != ChannelKind::Text {
        return Err(AppError::Validation("canais de voz não têm mensagens"));
    }
    Ok(access)
}

/// `GET /channels/{id}/messages?before=<id>|after=<id>&limit=50`
///
/// Sempre em ordem cronológica. Os cursores funcionam porque o UUIDv7 é ordenável por tempo.
pub async fn list(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(channel): Path<ChannelId>,
    Query(q): Query<MessagesQuery>,
) -> AppResult<Json<Vec<Message>>> {
    text_channel(&state, channel, user).await?;
    let limit = q.limit.unwrap_or(MESSAGES_PAGE_DEFAULT);
    if !(1..=MESSAGES_PAGE_MAX).contains(&limit) {
        return Err(AppError::Validation("limit precisa ficar entre 1 e 100"));
    }

    let rows: Vec<MessageRow> = match (q.before, q.after) {
        (Some(_), Some(_)) => {
            return Err(AppError::Validation("use before ou after, não os dois"));
        }
        (None, Some(after)) => {
            let sql = format!(
                "{SELECT_MESSAGE} WHERE m.channel_id = $1 AND m.id > $2 ORDER BY m.id LIMIT $3"
            );
            sqlx::query_as(&sql)
                .bind(channel.0)
                .bind(after.0)
                .bind(i64::from(limit))
                .fetch_all(&state.db)
                .await?
        }
        (before, None) => {
            // As mais novas primeiro para o LIMIT cortar as antigas; depois inverte.
            let sql = format!(
                "{SELECT_MESSAGE} WHERE m.channel_id = $1 AND ($2::uuid IS NULL OR m.id < $2)
                 ORDER BY m.id DESC LIMIT $3"
            );
            let mut rows: Vec<MessageRow> = sqlx::query_as(&sql)
                .bind(channel.0)
                .bind(before.map(|id| id.0))
                .bind(i64::from(limit))
                .fetch_all(&state.db)
                .await?;
            rows.reverse();
            rows
        }
    };
    Ok(Json(rows.into_iter().map(Message::from).collect()))
}

/// `POST /channels/{id}/messages`: grava a mensagem. O envio aos inscritos pelo WebSocket
/// entra junto com o WS.
pub async fn create(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(channel): Path<ChannelId>,
    Json(req): Json<CreateMessageRequest>,
) -> AppResult<(StatusCode, Json<Message>)> {
    text_channel(&state, channel, user).await?;
    validate_message_content(&req.content).map_err(AppError::Validation)?;

    let row: MessageRow = sqlx::query_as(
        "WITH m AS (
            INSERT INTO messages (id, channel_id, author_id, content) VALUES ($1, $2, $3, $4)
            RETURNING id, channel_id, author_id, content, created_at, edited_at
        )
        SELECT m.id, m.channel_id, m.content, m.created_at, m.edited_at,
            u.id AS author_id, u.username, u.display_name
        FROM m LEFT JOIN users u ON u.id = m.author_id",
    )
    .bind(Uuid::now_v7())
    .bind(channel.0)
    .bind(user.0)
    .bind(&req.content)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(row.into())))
}
