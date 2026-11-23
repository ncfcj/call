use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use proto::{
    ChannelId, ChannelKind, CreateMessageRequest, Message, MessageId, MessagesQuery, ServerEvent,
    User, UserId,
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

/// De onde a página do histórico começa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    Latest,
    Before(MessageId),
    After(MessageId),
}

/// Valida a query do histórico: `limit` de 1 a 100 (padrão 50) e no máximo um cursor.
fn page(q: &MessagesQuery) -> AppResult<(u32, Cursor)> {
    let limit = q.limit.unwrap_or(MESSAGES_PAGE_DEFAULT);
    if !(1..=MESSAGES_PAGE_MAX).contains(&limit) {
        return Err(AppError::Validation("limit precisa ficar entre 1 e 100"));
    }
    let cursor = match (q.before, q.after) {
        (Some(_), Some(_)) => return Err(AppError::Validation("use before ou after, não os dois")),
        (Some(before), None) => Cursor::Before(before),
        (None, Some(after)) => Cursor::After(after),
        (None, None) => Cursor::Latest,
    };
    Ok((limit, cursor))
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
    let (limit, cursor) = page(&q)?;

    let rows: Vec<MessageRow> = match cursor {
        Cursor::After(after) => {
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
        Cursor::Latest | Cursor::Before(_) => {
            let before = match cursor {
                Cursor::Before(id) => Some(id),
                _ => None,
            };
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

/// `POST /channels/{id}/messages`: grava a mensagem e depois a envia, como `MESSAGE_CREATE`,
/// a quem está inscrito no servidor pelo WebSocket. Gravar antes garante que um client que
/// reconecta acha no histórico tudo o que já foi difundido.
pub async fn create(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(channel): Path<ChannelId>,
    Json(req): Json<CreateMessageRequest>,
) -> AppResult<(StatusCode, Json<Message>)> {
    state.limits.message.check(user)?;
    let access = text_channel(&state, channel, user).await?;
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
    let message = Message::from(row);
    state.hub.publish(access.guild.guild_id, &ServerEvent::MessageCreate(message.clone()), None);
    Ok((StatusCode::CREATED, Json(message)))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn id(n: u128) -> MessageId {
        MessageId(Uuid::from_u128(n))
    }

    #[test]
    fn page_defaults_to_the_latest_50() {
        assert_eq!(page(&MessagesQuery::default()).unwrap(), (50, Cursor::Latest));
    }

    #[test]
    fn page_accepts_one_cursor_and_limits_from_1_to_100() {
        let q = MessagesQuery { before: Some(id(1)), after: None, limit: Some(1) };
        assert_eq!(page(&q).unwrap(), (1, Cursor::Before(id(1))));
        let q = MessagesQuery { before: None, after: Some(id(2)), limit: Some(100) };
        assert_eq!(page(&q).unwrap(), (100, Cursor::After(id(2))));
    }

    #[test]
    fn page_rejects_bad_limits_and_two_cursors() {
        for limit in [0, 101, u32::MAX] {
            let q = MessagesQuery { limit: Some(limit), ..Default::default() };
            assert!(matches!(page(&q), Err(AppError::Validation(_))), "limit {limit}");
        }
        let q = MessagesQuery { before: Some(id(1)), after: Some(id(2)), limit: None };
        assert!(matches!(page(&q), Err(AppError::Validation(_))));
    }

    fn row(author: (Option<u128>, Option<&str>, Option<&str>)) -> MessageRow {
        MessageRow {
            id: Uuid::from_u128(5),
            channel_id: Uuid::from_u128(6),
            content: "oi".into(),
            created_at: Utc.timestamp_opt(1_800_000_000, 0).unwrap(),
            edited_at: None,
            author_id: author.0.map(Uuid::from_u128),
            username: author.1.map(Into::into),
            display_name: author.2.map(Into::into),
        }
    }

    #[test]
    fn row_with_author_becomes_a_message() {
        let m = Message::from(row((Some(9), Some("ana"), Some("Ana"))));
        assert_eq!(m.id, id(5));
        assert_eq!(m.channel_id, ChannelId(Uuid::from_u128(6)));
        assert_eq!(m.content, "oi");
        let author = m.author.unwrap();
        assert_eq!(
            (author.id, author.username.as_str(), author.display_name.as_str()),
            (UserId(Uuid::from_u128(9)), "ana", "Ana")
        );
    }

    #[test]
    fn deleted_author_becomes_none() {
        assert!(Message::from(row((None, None, None))).author.is_none());
        // Linha incompleta (não deveria acontecer) também vira "conta apagada".
        assert!(Message::from(row((Some(9), None, Some("Ana")))).author.is_none());
    }
}
