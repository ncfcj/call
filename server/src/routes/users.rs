use axum::{Json, extract::State};
use proto::{User, UserId};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    error::{AppError, AppResult},
    state::AppState,
};

#[derive(sqlx::FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
}

impl From<UserRow> for User {
    fn from(row: UserRow) -> Self {
        Self { id: UserId(row.id), username: row.username, display_name: row.display_name }
    }
}

pub async fn find_user(db: &PgPool, id: UserId) -> AppResult<Option<User>> {
    let row: Option<UserRow> =
        sqlx::query_as("SELECT id, username, display_name FROM users WHERE id = $1")
            .bind(id.0)
            .fetch_optional(db)
            .await?;
    Ok(row.map(User::from))
}

/// `GET /users/@me`
pub async fn me(State(state): State<AppState>, AuthUser(id): AuthUser) -> AppResult<Json<User>> {
    // Token válido de um usuário que não existe mais: a sessão acabou.
    find_user(&state.db, id).await?.map(Json).ok_or(AppError::Unauthorized)
}
