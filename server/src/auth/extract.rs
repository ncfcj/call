use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts},
};
use proto::UserId;

use crate::{error::AppError, state::AppState};

/// Usuário autenticado pelo header `Authorization: Bearer <access_token>`.
/// Colocar `AuthUser` nos argumentos de um handler torna a rota protegida.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser(pub UserId);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, AppError> {
        let token = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;
        state.jwt.verify(token).map(AuthUser).ok_or(AppError::Unauthorized)
    }
}
