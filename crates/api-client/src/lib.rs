//! Cliente REST do call-server. Usa `reqwest`, que em wasm32 roda sobre o `fetch` do navegador.

use proto::HealthResponse;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("falha de rede: {0}")]
    Network(#[from] reqwest::Error),
    #[error("a API respondeu {0}")]
    Status(u16),
}

#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        Self { base_url, http: reqwest::Client::new() }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// `GET /health`
    pub async fn health(&self) -> Result<HealthResponse, ApiError> {
        let resp = self.http.get(format!("{}/health", self.base_url)).send().await?;
        if !resp.status().is_success() {
            return Err(ApiError::Status(resp.status().as_u16()));
        }
        Ok(resp.json().await?)
    }
}
