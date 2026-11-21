use axum::{Json, extract::State, http::StatusCode};
use axum_extra::extract::cookie::CookieJar;
use proto::{
    AuthResponse, LoginRequest, RegisterRequest, User, UserId,
    auth::{normalize_username, validate_display_name, validate_password, validate_username},
};
use uuid::Uuid;

use super::users::find_user;
use crate::{
    auth::{cookie, jwt::ACCESS_TTL_SECS, password, refresh},
    error::{AppError, AppResult},
    state::AppState,
};

type AuthReply = (CookieJar, Json<AuthResponse>);

/// Abre a sessão: refresh token no cookie, access token no corpo.
async fn open_session(state: &AppState, jar: CookieJar, user: User) -> AppResult<AuthReply> {
    let refresh_token = refresh::start_session(&state.db, user.id).await?;
    Ok((cookie::set_refresh(jar, refresh_token, state.cookie_secure), Json(response(state, user)?)))
}

fn response(state: &AppState, user: User) -> AppResult<AuthResponse> {
    Ok(AuthResponse { access_token: state.jwt.issue(user.id)?, expires_in: ACCESS_TTL_SECS, user })
}

/// `POST /auth/register`
pub async fn register(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<RegisterRequest>,
) -> AppResult<(StatusCode, CookieJar, Json<AuthResponse>)> {
    let username = normalize_username(&req.username);
    let display_name = req.display_name.trim().to_owned();
    validate_username(&username).map_err(AppError::Validation)?;
    validate_display_name(&display_name).map_err(AppError::Validation)?;
    validate_password(&req.password).map_err(AppError::Validation)?;

    let password_hash = password::hash(req.password).await?;
    let id = UserId(Uuid::now_v7());
    let inserted = sqlx::query(
        "INSERT INTO users (id, username, display_name, password_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(id.0)
    .bind(&username)
    .bind(&display_name)
    .bind(password_hash)
    .execute(&state.db)
    .await;
    match inserted {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AppError::UsernameTaken);
        }
        Err(e) => return Err(e.into()),
    }

    let (jar, body) = open_session(&state, jar, User { id, username, display_name }).await?;
    Ok((StatusCode::CREATED, jar, body))
}

#[derive(sqlx::FromRow)]
struct LoginRow {
    id: Uuid,
    username: String,
    display_name: String,
    password_hash: String,
}

/// `POST /auth/login`
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> AppResult<AuthReply> {
    let row: Option<LoginRow> = sqlx::query_as(
        "SELECT id, username, display_name, password_hash FROM users WHERE username = $1",
    )
    .bind(normalize_username(&req.username))
    .fetch_optional(&state.db)
    .await?;

    // Mesmo sem usuário, roda o Argon2 (contra um hash fictício) para não vazar pelo tempo.
    let stored = row.as_ref().map(|r| r.password_hash.clone());
    if !password::verify(req.password, stored).await? {
        return Err(AppError::InvalidCredentials);
    }
    let row = row.ok_or(AppError::InvalidCredentials)?;
    let user = User { id: UserId(row.id), username: row.username, display_name: row.display_name };
    open_session(&state, jar, user).await
}

/// `POST /auth/refresh`: troca o cookie de refresh por um novo e emite um access token.
pub async fn refresh(State(state): State<AppState>, jar: CookieJar) -> AppResult<AuthReply> {
    let token = cookie::read_refresh(&jar).ok_or(AppError::Unauthorized)?;
    // Em caso de 401 o cookie não é apagado: pode ser a corrida entre duas abas, e a outra
    // aba já gravou um cookie novo, que não pode ser apagado.
    let (user_id, next) = refresh::rotate(&state.db, &token).await?;
    let user = find_user(&state.db, user_id).await?.ok_or(AppError::Unauthorized)?;
    let body = response(&state, user)?;
    Ok((cookie::set_refresh(jar, next, state.cookie_secure), Json(body)))
}

/// `POST /auth/logout`: revoga a sessão do cookie e o apaga. Sempre responde 204.
pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> AppResult<(StatusCode, CookieJar)> {
    if let Some(token) = cookie::read_refresh(&jar) {
        refresh::end_session(&state.db, &token).await?;
    }
    Ok((StatusCode::NO_CONTENT, cookie::clear_refresh(jar)))
}
