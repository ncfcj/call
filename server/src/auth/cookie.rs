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
