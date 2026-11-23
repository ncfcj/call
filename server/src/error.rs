use axum::{
    Json,
    http::{StatusCode, header},
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
    #[error("muitas tentativas; espere {retry_after} s e tente de novo")]
    RateLimited { retry_after: u128 },
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
            Self::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, ErrorCode::RateLimited),
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
        let mut res = (status, Json(ErrorResponse { code, message })).into_response();
        if let Self::RateLimited { retry_after } = self
            && let Ok(value) = retry_after.to_string().parse()
        {
            res.headers_mut().insert(header::RETRY_AFTER, value);
        }
        res
    }
}

/// Os extratores e o roteador do axum recusam com texto puro (JSON malformado, UUID inválido,
/// corpo grande, rota inexistente). Troca o corpo pelo mesmo `{code, message}` dos handlers.
pub async fn json_errors(mut res: Response) -> Response {
    let status = res.status();
    let is_json = res.headers().get(header::CONTENT_TYPE).is_some_and(|v| v == "application/json");
    if is_json || !(status.is_client_error() || status.is_server_error()) {
        return res;
    }
    let (code, message) = match status {
        StatusCode::NOT_FOUND => (ErrorCode::NotFound, "não encontrado"),
        StatusCode::PAYLOAD_TOO_LARGE => (ErrorCode::Validation, "requisição grande demais"),
        s if s.is_server_error() => (ErrorCode::Internal, "erro interno do servidor"),
        _ => (ErrorCode::Validation, "requisição inválida"),
    };
    let body = Json(ErrorResponse { code, message: message.to_owned() });
    res.headers_mut().remove(header::CONTENT_LENGTH);
    let (parts, body) = body.into_response().into_parts();
    res.headers_mut().extend(parts.headers);
    *res.body_mut() = body;
    res
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    async fn body(err: AppError) -> (StatusCode, serde_json::Value) {
        let res = err.into_response();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn each_error_has_its_status_and_code() {
        let cases = [
            (AppError::Validation("x"), StatusCode::UNPROCESSABLE_ENTITY, "validation"),
            (AppError::UsernameTaken, StatusCode::CONFLICT, "username_taken"),
            (AppError::InvalidCredentials, StatusCode::UNAUTHORIZED, "invalid_credentials"),
            (AppError::Unauthorized, StatusCode::UNAUTHORIZED, "unauthorized"),
            (AppError::Forbidden("y"), StatusCode::FORBIDDEN, "forbidden"),
            (AppError::NotFound, StatusCode::NOT_FOUND, "not_found"),
            (
                AppError::RateLimited { retry_after: 7 },
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
            ),
        ];
        for (err, status, code) in cases {
            let message = err.to_string();
            let (got_status, json) = body(err).await;
            assert_eq!(got_status, status, "{code}");
            assert_eq!(json["code"], code);
            assert_eq!(json["message"], message, "a mensagem é a do erro");
        }
    }

    #[tokio::test]
    async fn plain_text_server_errors_become_internal() {
        let res = json_errors((StatusCode::BAD_GATEWAY, "detalhe").into_response()).await;
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["code"], "internal");
        assert_eq!(json["message"], "erro interno do servidor");
    }

    #[tokio::test]
    async fn internal_errors_hide_the_detail() {
        let err = AppError::Internal(anyhow::anyhow!("senha do banco: hunter2"));
        assert_eq!(err.code(), ErrorCode::Internal);
        let (status, json) = body(err).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json["message"], "erro interno do servidor");
        assert!(!json.to_string().contains("hunter2"));
    }

    #[test]
    fn database_errors_become_internal() {
        let err = AppError::from(sqlx::Error::RowNotFound);
        assert_eq!(err.code(), ErrorCode::Internal);
    }

    #[test]
    fn validation_message_is_shown_as_is() {
        assert_eq!(AppError::Validation("nome curto").to_string(), "nome curto");
        assert_eq!(AppError::Forbidden("só o dono").to_string(), "só o dono");
    }
}
