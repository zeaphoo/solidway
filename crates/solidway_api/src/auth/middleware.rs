use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::cookie::CookieJar;

use crate::db::users::User;
use crate::error::{ApiError, Result};
use crate::state::{now_unix, AppState};

use super::session::{hash_token, COOKIE_NAME};

/// 注入到请求扩展中的当前登录用户。
#[derive(Clone, Debug)]
pub struct CurrentUser(pub User);

/// 认证中间件：校验会话 Cookie，失败返回 401。
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let jar = CookieJar::from_headers(req.headers());
    let token = jar
        .get(COOKIE_NAME)
        .map(|c| c.value().to_string())
        .ok_or(ApiError::Unauthorized)?;
    let token_hash = hash_token(&token);
    let user = crate::db::sessions::find_user(&state.pool, &token_hash, now_unix())
        .await?
        .ok_or(ApiError::Unauthorized)?;
    req.extensions_mut().insert(CurrentUser(user));
    Ok(next.run(req).await)
}

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self> {
        parts
            .extensions
            .get::<CurrentUser>()
            .cloned()
            .ok_or(ApiError::Unauthorized)
    }
}
