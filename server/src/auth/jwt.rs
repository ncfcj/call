use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use proto::UserId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Validade do access token. Curta de propósito: o refresh token cobre a sessão longa.
pub const ACCESS_TTL_SECS: u64 = 15 * 60;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: Uuid,
    iat: i64,
    exp: i64,
}

pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
}

impl JwtKeys {
    pub fn new(secret: &[u8]) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 5;
        validation.set_required_spec_claims(&["exp", "sub"]);
        // Só falha com segredo vazio; o Config já exige pelo menos 32 bytes.
        let invalid = "JWT_SECRET inválido";
        Self {
            encoding: EncodingKey::from_secret(secret).expect(invalid),
            decoding: DecodingKey::from_secret(secret).expect(invalid),
            validation,
        }
    }

    pub fn issue(&self, user: UserId) -> anyhow::Result<String> {
        let now = Utc::now().timestamp();
        let claims =
            Claims { sub: user.0, iat: now, exp: now.saturating_add_unsigned(ACCESS_TTL_SECS) };
        Ok(jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)?)
    }

    /// `None` para token inválido, adulterado ou expirado.
    pub fn verify(&self, token: &str) -> Option<UserId> {
        jsonwebtoken::decode::<Claims>(token, &self.decoding, &self.validation)
            .ok()
            .map(|data| UserId(data.claims.sub))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"segredo-de-teste-com-mais-de-32-bytes";

    #[test]
    fn roundtrip() {
        let keys = JwtKeys::new(SECRET);
        let user = UserId(Uuid::now_v7());
        let token = keys.issue(user).unwrap();
        assert_eq!(keys.verify(&token), Some(user));
    }

    #[test]
    fn rejects_other_secret_and_garbage() {
        let token = JwtKeys::new(SECRET).issue(UserId(Uuid::now_v7())).unwrap();
        assert_eq!(JwtKeys::new(b"outro-segredo-com-mais-de-32-bytes!!").verify(&token), None);
        assert_eq!(JwtKeys::new(SECRET).verify("nao.e.jwt"), None);
    }

    #[test]
    fn rejects_expired() {
        let keys = JwtKeys::new(SECRET);
        let past = Utc::now().timestamp() - 3600;
        let claims = Claims { sub: Uuid::now_v7(), iat: past - 60, exp: past };
        let token =
            jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &keys.encoding).unwrap();
        assert_eq!(keys.verify(&token), None);
    }

    #[test]
    fn access_token_lasts_fifteen_minutes() {
        let keys = JwtKeys::new(SECRET);
        let token = keys.issue(UserId(Uuid::now_v7())).unwrap();
        let mut validation = Validation::new(Algorithm::HS256);
        validation.validate_exp = false;
        let claims =
            jsonwebtoken::decode::<Claims>(&token, &keys.decoding, &validation).unwrap().claims;
        assert_eq!(ACCESS_TTL_SECS, 15 * 60);
        assert_eq!(claims.exp - claims.iat, 15 * 60);
        assert!((claims.iat - Utc::now().timestamp()).abs() <= 5, "emitido agora");
    }
}
