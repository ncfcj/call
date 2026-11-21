//! Autenticação: corpos de REST e regras de validação.
//!
//! As regras ficam aqui para o client validar o formulário com as mesmas regras do servidor.
//! O servidor sempre valida de novo.

use serde::{Deserialize, Serialize};

use crate::UserId;

/// Usuário como os outros o veem. Nunca inclui o hash da senha.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
}

/// `POST /auth/register`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
}

/// `POST /auth/login`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Resposta de register, login e refresh. O refresh token vai num cookie `HttpOnly`, nunca no corpo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    /// Segundos até o `access_token` expirar.
    pub expires_in: u64,
    pub user: User,
}

pub const USERNAME_LEN: std::ops::RangeInclusive<usize> = 2..=32;
pub const DISPLAY_NAME_LEN: std::ops::RangeInclusive<usize> = 1..=64;
pub const PASSWORD_LEN: std::ops::RangeInclusive<usize> = 8..=128;

/// Normaliza o username: sem espaços nas pontas e em minúsculas.
pub fn normalize_username(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Valida um username já normalizado.
pub fn validate_username(username: &str) -> Result<(), &'static str> {
    if !USERNAME_LEN.contains(&username.chars().count()) {
        return Err("o nome de usuário precisa ter de 2 a 32 caracteres");
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
    {
        return Err("o nome de usuário aceita só letras minúsculas, números, _ e .");
    }
    Ok(())
}

pub fn validate_display_name(display_name: &str) -> Result<(), &'static str> {
    if !DISPLAY_NAME_LEN.contains(&display_name.trim().chars().count()) {
        return Err("o nome de exibição precisa ter de 1 a 64 caracteres");
    }
    Ok(())
}

pub fn validate_password(password: &str) -> Result<(), &'static str> {
    if !PASSWORD_LEN.contains(&password.chars().count()) {
        return Err("a senha precisa ter de 8 a 128 caracteres");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_rules() {
        assert_eq!(normalize_username("  Nilton "), "nilton");
        assert!(validate_username("nilton_c.1").is_ok());
        assert!(validate_username("a").is_err());
        assert!(validate_username(&"a".repeat(33)).is_err());
        assert!(validate_username("com espaço").is_err());
        assert!(validate_username("Maiuscula").is_err());
    }

    #[test]
    fn display_name_and_password_rules() {
        assert!(validate_display_name("Nilton Costa").is_ok());
        assert!(validate_display_name("   ").is_err());
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password("1234567").is_err());
    }
}
