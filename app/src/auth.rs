//! Single-user login. Credentials come from the environment; sessions are random tokens kept in
//! memory (a restart signs everyone out) and travel in an HttpOnly cookie.
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{config::Secret, routers::AppState};

const COOKIE: &str = "vw_session";
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
/// Slows password guessing: every failed attempt waits this long before answering.
const FAIL_DELAY: Duration = Duration::from_millis(600);

pub struct Auth {
    user: String,
    password: Secret,
    sessions: Mutex<HashMap<String, Instant>>,
}

impl Auth {
    /// `None` when no password is configured: login is then switched off (local development).
    pub fn from_config(user: &str, password: Option<&Secret>) -> Option<Arc<Auth>> {
        let password = password.filter(|p| !p.expose().is_empty())?;
        Some(Arc::new(Auth {
            user: user.trim().to_string(),
            password: password.clone(),
            sessions: Mutex::new(HashMap::new()),
        }))
    }

    fn check(&self, user: &str, password: &str) -> bool {
        // evaluate both so the answer time does not reveal which one was wrong
        let u = ct_eq(user.as_bytes(), self.user.as_bytes());
        let p = ct_eq(password.as_bytes(), self.password.expose().as_bytes());
        u & p
    }

    fn open_session(&self) -> String {
        let token = format!("{:032x}{:032x}", rand::random::<u128>(), rand::random::<u128>());
        let now = Instant::now();
        let mut s = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        s.retain(|_, exp| *exp > now);
        s.insert(token.clone(), now + SESSION_TTL);
        token
    }

    fn valid(&self, token: &str) -> bool {
        let s = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        s.get(token).is_some_and(|exp| *exp > Instant::now())
    }

    fn close(&self, token: &str) {
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(token);
    }
}

/// Constant-time byte comparison (length mismatch still compares every byte of `a`).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    for (i, x) in a.iter().enumerate() {
        diff |= x ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.to_string())
}

fn is_https(headers: &HeaderMap) -> bool {
    headers.get("x-forwarded-proto").is_some_and(|v| v.as_bytes() == b"https")
}

fn cookie(value: &str, max_age: u64, headers: &HeaderMap) -> HeaderValue {
    let secure = if is_https(headers) { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{COOKIE}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"))
        .expect("cookie is ASCII")
}

fn authed(auth: &Auth, headers: &HeaderMap) -> bool {
    session_token(headers).is_some_and(|t| auth.valid(&t))
}

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

pub async fn login(State(s): State<AppState>, headers: HeaderMap, Json(c): Json<Credentials>) -> Response {
    let Some(auth) = &s.auth else {
        return Json(json!({ "auth_required": false, "authenticated": true })).into_response();
    };
    if !auth.check(&c.username, &c.password) {
        tokio::time::sleep(FAIL_DELAY).await;
        return (StatusCode::UNAUTHORIZED, "Wrong username or password.").into_response();
    }
    let token = auth.open_session();
    (
        [(header::SET_COOKIE, cookie(&token, SESSION_TTL.as_secs(), &headers))],
        Json(json!({ "auth_required": true, "authenticated": true, "user": auth.user })),
    )
        .into_response()
}

pub async fn logout(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if let (Some(auth), Some(t)) = (&s.auth, session_token(&headers)) {
        auth.close(&t);
    }
    ([(header::SET_COOKIE, cookie("", 0, &headers))], StatusCode::NO_CONTENT).into_response()
}

/// Always 200, so the UI can ask "who am I" without an error in the console.
pub async fn me(State(s): State<AppState>, headers: HeaderMap) -> Json<serde_json::Value> {
    match &s.auth {
        None => Json(json!({ "auth_required": false, "authenticated": true })),
        Some(a) if authed(a, &headers) => {
            Json(json!({ "auth_required": true, "authenticated": true, "user": a.user }))
        }
        Some(_) => Json(json!({ "auth_required": true, "authenticated": false })),
    }
}

/// Guards every route it wraps: 401 without a valid session. A no-op when login is off.
pub async fn require(State(s): State<AppState>, req: Request, next: Next) -> Response {
    match &s.auth {
        Some(a) if !authed(a, req.headers()) => (StatusCode::UNAUTHORIZED, "Sign in required.").into_response(),
        _ => next.run(req).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> Arc<Auth> {
        Auth::from_config("admin", Some(&"s3cret".parse().unwrap())).unwrap()
    }

    #[test]
    fn empty_password_turns_login_off() {
        assert!(Auth::from_config("admin", None).is_none());
        assert!(Auth::from_config("admin", Some(&"".parse().unwrap())).is_none());
    }

    #[test]
    fn checks_both_fields() {
        let a = auth();
        assert!(a.check("admin", "s3cret"));
        assert!(!a.check("admin", "s3cre"));
        assert!(!a.check("root", "s3cret"));
        assert!(!a.check("admin", "s3cretX"));
    }

    #[test]
    fn sessions_open_and_close() {
        let a = auth();
        let t = a.open_session();
        assert_eq!(t.len(), 64);
        assert!(a.valid(&t));
        assert!(!a.valid("nope"));
        a.close(&t);
        assert!(!a.valid(&t));
    }

    #[test]
    fn reads_the_session_cookie_among_others() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("a=1; vw_session=abc; b=2"));
        assert_eq!(session_token(&h).as_deref(), Some("abc"));
        assert_eq!(session_token(&HeaderMap::new()), None);
    }

    #[test]
    fn secure_flag_follows_the_proxy_header() {
        let mut h = HeaderMap::new();
        assert!(!cookie("x", 1, &h).to_str().unwrap().contains("Secure"));
        h.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(cookie("x", 1, &h).to_str().unwrap().contains("Secure"));
    }
}
