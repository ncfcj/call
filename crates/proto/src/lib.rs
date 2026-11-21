//! Contrato entre o servidor e o client.
//!
//! Este crate é compilado tanto para o servidor (nativo) quanto para o client
//! (wasm32). Mantenha-o sem dependências de runtime: só tipos e `serde`.

pub mod auth;
pub mod error;
pub mod health;
pub mod ids;
pub mod ws;

pub use auth::{AuthResponse, LoginRequest, RegisterRequest, User};
pub use error::{ErrorCode, ErrorResponse};
pub use health::{HealthResponse, HealthStatus};
pub use ids::{ChannelId, GuildId, MessageId, UserId};
