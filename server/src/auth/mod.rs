//! Autenticação: senha (Argon2), access token (JWT de 15 min) e refresh token rotativo.
//!
//! Fluxo:
//! 1. `register` / `login` validam a senha e abrem uma família de refresh tokens.
//! 2. O access token vai no corpo; o refresh vai num cookie `HttpOnly` com `Path=/auth`.
//! 3. `refresh` troca o refresh token por outro (rotação) e emite um access token novo.
//! 4. Rotas protegidas recebem o usuário pelo extrator [`AuthUser`].

pub mod cookie;
pub mod extract;
pub mod jwt;
pub mod password;
pub mod refresh;

pub use extract::AuthUser;
