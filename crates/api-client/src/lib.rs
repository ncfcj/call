//! Cliente REST e WebSocket do call-server. Usa `reqwest`, que em wasm32 roda sobre o `fetch`
//! do navegador.
//!
//! O access token fica só na memória. O refresh token fica no cookie `HttpOnly` que o servidor
//! define, e o JavaScript nunca o lê: por isso toda chamada vai com `credentials: include`.
//! Quando a API responde 401, o cliente tenta um refresh e repete a chamada uma vez.
//!
//! Fora do navegador (o app desktop), o próprio cliente guarda os cookies, e
//! [`ApiClient::refresh_cookie`] e [`ApiClient::restore_refresh_cookie`] levam o de refresh
//! para fora e de volta, para a sessão sobreviver a fechar o app.

pub mod ws;

use std::sync::{Arc, Mutex};

use proto::{
    AuthResponse, Channel, ChannelId, CreateChannelRequest, CreateGuildRequest,
    CreateMessageRequest, ErrorResponse, Guild, GuildId, HealthResponse, LoginRequest, Member,
    Message, MessagesQuery, RegisterRequest, User, VoiceTokenResponse, WsTicketResponse,
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
    #[cfg(not(target_arch = "wasm32"))]
    cookies: Arc<reqwest::cookie::Jar>,
}

impl ApiClient {
    #[cfg(target_arch = "wasm32")]
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        Self { base_url, http: reqwest::Client::new(), token: Arc::default() }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        let cookies = Arc::new(reqwest::cookie::Jar::default());
        let http = reqwest::Client::builder()
            .cookie_provider(cookies.clone())
            .build()
            .expect("cliente HTTP com a configuração padrão");
        Self { base_url, http, token: Arc::default(), cookies }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn auth_url(&self) -> Option<reqwest::Url> {
        format!("{}/auth/refresh", self.base_url).parse().ok()
    }

    /// O cookie de refresh atual (`refresh_token=...`), para guardar fora do app. `None` sem
    /// sessão: o logout e um refresh recusado o apagam.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn refresh_cookie(&self) -> Option<String> {
        use reqwest::cookie::CookieStore;
        let value = self.cookies.cookies(&self.auth_url()?)?;
        value.to_str().ok().map(str::to_owned)
    }

    /// Devolve ao cliente um cookie lido de [`Self::refresh_cookie`]; depois, chame
    /// [`Self::refresh`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn restore_refresh_cookie(&self, cookie: &str) {
        if let Some(url) = self.auth_url() {
            self.cookies.add_cookie_str(&format!("{cookie}; Path=/auth"), &url);
        }
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
        self.get(&messages_path(channel, q)).await
    }

    pub async fn send_message(&self, channel: ChannelId, content: &str) -> ApiResult<Message> {
        let body = CreateMessageRequest { content: content.to_owned() };
        self.json(Method::POST, &format!("/channels/{channel}/messages"), Some(&body)).await
    }

    // --- WebSocket --------------------------------------------------------------------------

    // --- Voz ---------------------------------------------------------------------------------

    /// `POST /channels/{id}/voice/token`
    pub async fn voice_token(&self, channel: ChannelId) -> ApiResult<VoiceTokenResponse> {
        self.json(Method::POST, &format!("/channels/{channel}/voice/token"), None::<&()>).await
    }

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

/// `/channels/{id}/messages` com a query só dos campos presentes.
fn messages_path(channel: ChannelId, q: &MessagesQuery) -> String {
    let mut params = Vec::new();
    if let Some(before) = q.before {
        params.push(format!("before={before}"));
    }
    if let Some(after) = q.after {
        params.push(format!("after={after}"));
    }
    if let Some(limit) = q.limit {
        params.push(format!("limit={limit}"));
    }
    let path = format!("/channels/{channel}/messages");
    if params.is_empty() { path } else { format!("{path}?{}", params.join("&")) }
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

    #[test]
    fn base_url_loses_the_trailing_slash() {
        assert_eq!(ApiClient::new("http://x:3000///").base_url(), "http://x:3000");
    }

    #[test]
    fn messages_path_has_only_the_given_fields() {
        let channel = ChannelId(uuid::Uuid::nil());
        let base = "/channels/00000000-0000-0000-0000-000000000000/messages";
        assert_eq!(messages_path(channel, &MessagesQuery::default()), base);
        let id = proto::MessageId(uuid::Uuid::from_u128(1));
        let q = MessagesQuery { before: Some(id), after: None, limit: Some(50) };
        assert_eq!(
            messages_path(channel, &q),
            format!("{base}?before=00000000-0000-0000-0000-000000000001&limit=50")
        );
        let q = MessagesQuery { before: None, after: Some(id), limit: None };
        assert_eq!(
            messages_path(channel, &q),
            format!("{base}?after=00000000-0000-0000-0000-000000000001")
        );
    }

    #[test]
    fn only_401_means_the_session_ended() {
        let api = |status, code| ApiError::Api {
            status,
            body: ErrorResponse { code, message: "x".into() },
        };
        assert!(api(401, proto::ErrorCode::Unauthorized).is_unauthorized());
        assert!(ApiError::Status(401).is_unauthorized());
        assert!(!api(403, proto::ErrorCode::Forbidden).is_unauthorized());
        assert!(!ApiError::Status(500).is_unauthorized());
    }

    #[test]
    fn api_error_shows_the_server_message() {
        let err = ApiError::Api {
            status: 422,
            body: ErrorResponse {
                code: proto::ErrorCode::Validation,
                message: "nome curto".into(),
            },
        };
        assert_eq!(err.to_string(), "nome curto");
        assert_eq!(ApiError::Status(502).to_string(), "a API respondeu 502");
    }
}
