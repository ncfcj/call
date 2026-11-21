use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use proto::{ErrorCode, ErrorResponse};

/// Erro de um handler. Vira uma resposta JSON `ErrorResponse` com o status HTTP certo.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Validation(&'static str),
    #[error("esse nome de usuário já está em uso")]
    UsernameTaken,
    #[error("usuário ou senha incorretos")]
    InvalidCredentials,
    #[error("sessão inválida ou expirada")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("não encontrado")]
    NotFound,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal(err.into())
    }
}

impl AppError {
    /// Código do erro, para respostas HTTP e eventos `ERROR` do WebSocket.
    pub fn code(&self) -> ErrorCode {
        self.status_and_code().1
    }

    fn status_and_code(&self) -> (StatusCode, ErrorCode) {
        match self {
            Self::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, ErrorCode::Validation),
            Self::UsernameTaken => (StatusCode::CONFLICT, ErrorCode::UsernameTaken),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, ErrorCode::InvalidCredentials),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, ErrorCode::Unauthorized),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, ErrorCode::Forbidden),
            Self::NotFound => (StatusCode::NOT_FOUND, ErrorCode::NotFound),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, ErrorCode::Internal),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let message = match &self {
            Self::Internal(err) => {
                // O detalhe fica no log; o client recebe uma mensagem genérica.
                tracing::error!(error = ?err, "erro interno");
                "erro interno do servidor".to_owned()
            }
            other => other.to_string(),
        };
        (status, Json(ErrorResponse { code, message })).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
