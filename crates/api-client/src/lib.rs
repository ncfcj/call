//! Cliente REST e WebSocket do call-server. Usa `reqwest`, que em wasm32 roda sobre o `fetch`
//! do navegador.
//!
//! O access token fica só na memória. O refresh token fica no cookie `HttpOnly` que o servidor
//! define, e o JavaScript nunca o lê: por isso toda chamada vai com `credentials: include`.
//! Quando a API responde 401, o cliente tenta um refresh e repete a chamada uma vez.

#[cfg(target_arch = "wasm32")]
pub mod ws;

use std::sync::{Arc, Mutex};

use proto::{
    AuthResponse, Channel, ChannelId, CreateChannelRequest, CreateGuildRequest,
    CreateMessageRequest, ErrorResponse, Guild, GuildId, HealthResponse, LoginRequest, Member,
    Message, MessagesQuery, RegisterRequest, User, WsTicketResponse,
};
use reqwest::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("falha de rede: {0}")]
    Network(#[from] reqwest::Error),
    /// A API recusou o pedido; `message` é o texto para mostrar ao usuário.
    #[error("{}", .body.message)]
    Api { status: u16, body: ErrorResponse },
    #[error("a API respondeu {0}")]
    Status(u16),
}

impl ApiError {
    /// A sessão acabou (refresh recusado): a UI deve voltar para o login.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Api { status: 401, .. } | Self::Status(401))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone)]
pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
    token: Arc<Mutex<Option<String>>>,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        Self { base_url, http: reqwest::Client::new(), token: Arc::default() }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn set_token(&self, token: Option<String>) {
        *self.token.lock().unwrap() = token;
    }

    fn token(&self) -> Option<String> {
        self.token.lock().unwrap().clone()
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let req = self.http.request(method, format!("{}{path}", self.base_url));
        #[cfg(target_arch = "wasm32")]
        let req = req.fetch_credentials_include();
        req
    }

    /// Envia e devolve a resposta bruta. Erros HTTP viram `ApiError::Api`.
    async fn send_once(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + ?Sized)>,
    ) -> ApiResult<reqwest::Response> {
        let mut req = self.request(method, path);
        if let Some(token) = self.token() {
            req = req.bearer_auth(token);
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status().as_u16();
        match resp.json::<ErrorResponse>().await {
            Ok(body) => Err(ApiError::Api { status, body }),
            Err(_) => Err(ApiError::Status(status)),
        }
    }

    /// Chamada autenticada: num 401, tenta um refresh e repete uma vez.
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + ?Sized)>,
    ) -> ApiResult<reqwest::Response> {
        match self.send_once(method.clone(), path, body).await {
            Err(err) if err.is_unauthorized() && self.token().is_some() => {
                self.refresh().await?;
                self.send_once(method, path, body).await
            }
            other => other,
        }
    }

    async fn json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&(impl Serialize + ?Sized)>,
    ) -> ApiResult<T> {
        Ok(self.send(method, path, body).await?.json().await?)
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> ApiResult<T> {
        self.json(Method::GET, path, None::<&()>).await
    }

    /// `GET /health`
    pub async fn health(&self) -> ApiResult<HealthResponse> {
        self.get("/health").await
    }

    // --- Autenticação -----------------------------------------------------------------------

    fn open_session(&self, auth: AuthResponse) -> User {
        self.set_token(Some(auth.access_token));
        auth.user
    }

    /// `POST /auth/register`
    pub async fn register(&self, req: &RegisterRequest) -> ApiResult<User> {
        let auth = self.send_once(Method::POST, "/auth/register", Some(req)).await?.json().await?;
        Ok(self.open_session(auth))
    }

    /// `POST /auth/login`
    pub async fn login(&self, req: &LoginRequest) -> ApiResult<User> {
        let auth = self.send_once(Method::POST, "/auth/login", Some(req)).await?.json().await?;
        Ok(self.open_session(auth))
    }

    /// `POST /auth/refresh`: troca o cookie por um access token novo. Ao abrir o app, é o que
    /// diz se ainda existe uma sessão.
    pub async fn refresh(&self) -> ApiResult<User> {
        let resp = self.request(Method::POST, "/auth/refresh").send().await?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            self.set_token(None);
            return Err(ApiError::Status(401));
        }
        if !resp.status().is_success() {
            return Err(ApiError::Status(resp.status().as_u16()));
        }
        Ok(self.open_session(resp.json().await?))
    }

    /// `POST /auth/logout`
    pub async fn logout(&self) -> ApiResult<()> {
        let result = self.send_once(Method::POST, "/auth/logout", None::<&()>).await;
        self.set_token(None);
        result.map(drop)
    }

    // --- Servidores e canais ----------------------------------------------------------------

    pub async fn my_guilds(&self) -> ApiResult<Vec<Guild>> {
        self.get("/users/@me/guilds").await
    }

    pub async fn create_guild(&self, name: &str) -> ApiResult<Guild> {
        let body = CreateGuildRequest { name: name.to_owned() };
        self.json(Method::POST, "/guilds", Some(&body)).await
    }

    pub async fn join_invite(&self, code: &str) -> ApiResult<Guild> {
        self.json(Method::POST, &format!("/invites/{}", code.trim()), None::<&()>).await
    }

    pub async fn leave_guild(&self, guild: GuildId) -> ApiResult<()> {
        self.send(Method::DELETE, &format!("/guilds/{guild}/members/@me"), None::<&()>)
            .await
            .map(drop)
    }

    pub async fn members(&self, guild: GuildId) -> ApiResult<Vec<Member>> {
        self.get(&format!("/guilds/{guild}/members")).await
    }

    pub async fn channels(&self, guild: GuildId) -> ApiResult<Vec<Channel>> {
        self.get(&format!("/guilds/{guild}/channels")).await
    }

    pub async fn create_channel(
        &self,
        guild: GuildId,
        req: &CreateChannelRequest,
    ) -> ApiResult<Channel> {
        self.json(Method::POST, &format!("/guilds/{guild}/channels"), Some(req)).await
    }

    // --- Mensagens --------------------------------------------------------------------------

    pub async fn messages(&self, channel: ChannelId, q: &MessagesQuery) -> ApiResult<Vec<Message>> {
        let mut path = format!("/channels/{channel}/messages?");
        if let Some(before) = q.before {
            path.push_str(&format!("before={before}&"));
        }
        if let Some(after) = q.after {
            path.push_str(&format!("after={after}&"));
        }
        if let Some(limit) = q.limit {
            path.push_str(&format!("limit={limit}"));
        }
        self.get(&path).await
    }

    pub async fn send_message(&self, channel: ChannelId, content: &str) -> ApiResult<Message> {
        let body = CreateMessageRequest { content: content.to_owned() };
        self.json(Method::POST, &format!("/channels/{channel}/messages"), Some(&body)).await
    }

    // --- WebSocket --------------------------------------------------------------------------

    /// `POST /ws/ticket`
    pub async fn ws_ticket(&self) -> ApiResult<WsTicketResponse> {
        self.json(Method::POST, "/ws/ticket", None::<&()>).await
    }

    /// URL do WebSocket com o ticket: `http` vira `ws` e `https` vira `wss`.
    pub fn ws_url(&self, ticket: &str) -> String {
        let base = self.base_url.replacen("http", "ws", 1);
        format!("{base}/ws?ticket={ticket}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_follows_the_api_scheme() {
        assert_eq!(
            ApiClient::new("http://localhost:3000/").ws_url("t"),
            "ws://localhost:3000/ws?ticket=t"
        );
        assert_eq!(ApiClient::new("https://api.x.dev").ws_url("t"), "wss://api.x.dev/ws?ticket=t");
    }
}
