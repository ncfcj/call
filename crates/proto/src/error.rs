use serde::{Deserialize, Serialize};

/// Corpo de toda resposta de erro da API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: ErrorCode,
    /// Texto para mostrar ao usuário.
    pub message: String,
}

/// Código estável para o client decidir o que fazer; a `message` pode mudar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Validation,
    UsernameTaken,
    InvalidCredentials,
    Unauthorized,
    /// Autenticado, mas sem permissão (por exemplo, quem não é dono tentando apagar o servidor).
    Forbidden,
    NotFound,
    /// Requisições demais; o header `Retry-After` diz quantos segundos esperar.
    RateLimited,
    Internal,
    /// Código que esta versão do client ainda não conhece.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_code_does_not_break_old_clients() {
        let body = r#"{"code":"codigo_do_futuro","message":"x"}"#;
        let err: ErrorResponse = serde_json::from_str(body).unwrap();
        assert_eq!(err.code, ErrorCode::Unknown);
    }
}
