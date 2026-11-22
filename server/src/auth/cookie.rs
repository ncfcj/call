use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use cookie::time::Duration;

use super::refresh::REFRESH_TTL_DAYS;

pub const REFRESH_COOKIE: &str = "refresh_token";

/// O cookie só é enviado para `/auth/*`, então não vaza para as outras rotas nem para o WS.
const COOKIE_PATH: &str = "/auth";

pub fn set_refresh(jar: CookieJar, token: String, secure: bool) -> CookieJar {
    jar.add(
        Cookie::build((REFRESH_COOKIE, token))
            .path(COOKIE_PATH)
            .http_only(true)
            .secure(secure)
            .same_site(SameSite::Lax)
            .max_age(Duration::days(REFRESH_TTL_DAYS)),
    )
}

pub fn clear_refresh(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::build(REFRESH_COOKIE).path(COOKIE_PATH))
}

pub fn read_refresh(jar: &CookieJar) -> Option<String> {
    jar.get(REFRESH_COOKIE).map(|c| c.value().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_cookie_is_locked_down() {
        let jar = set_refresh(CookieJar::new(), "abc".into(), true);
        let c = jar.get(REFRESH_COOKIE).unwrap();
        assert_eq!(c.value(), "abc");
        assert_eq!(c.path(), Some("/auth"), "só vai para /auth/*");
        assert_eq!(c.http_only(), Some(true), "o JavaScript não lê");
        assert_eq!(c.secure(), Some(true));
        assert_eq!(c.same_site(), Some(SameSite::Lax));
        assert_eq!(c.max_age(), Some(Duration::days(30)));
        assert_eq!(read_refresh(&jar).as_deref(), Some("abc"));
    }

    #[test]
    fn secure_can_be_off_in_development() {
        let jar = set_refresh(CookieJar::new(), "abc".into(), false);
        assert_eq!(jar.get(REFRESH_COOKIE).unwrap().secure(), Some(false));
    }

    #[test]
    fn clearing_removes_the_cookie() {
        let jar = clear_refresh(set_refresh(CookieJar::new(), "abc".into(), true));
        assert_eq!(read_refresh(&jar), None);
        assert_eq!(read_refresh(&CookieJar::new()), None);
    }
}
