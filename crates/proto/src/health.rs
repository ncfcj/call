use serde::{Deserialize, Serialize};

/// Resposta de `GET /health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub version: String,
    /// `true` quando o servidor conseguiu falar com o Postgres.
    pub database: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ok,
    Degraded,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_wire_format() {
        let json = r#"{"status":"degraded","version":"0.1.0","database":false}"#;
        let h: HealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(h.status, HealthStatus::Degraded);
        assert!(!h.database);
        assert_eq!(serde_json::to_string(&h).unwrap(), json);
    }
}
