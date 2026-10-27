use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use serde::{Deserialize, Serialize};
use time::Duration;

use super::middleware::CurrentUser;
use super::password::{self, hash_password_async, is_locked, verify_password_async};
use super::session::{self, COOKIE_NAME};
use crate::ADMIN_USERNAME;
use crate::db::{sessions, users};
use crate::db::users::User;
use crate::error::{ApiError, Result};
use crate::state::{now_unix, AppState};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct MeResponse {
    pub id: String,
    pub username: String,
    pub role: String,
    pub display_name: Option<String>,
    pub title: Option<String>,
    /// 是否已设置口令（锁定态 = 未设置）。界面据此区分「设置口令」与「修改口令」。
    pub has_password: bool,
}

impl From<User> for MeResponse {
    fn from(user: User) -> Self {
        Self {
            has_password: !is_locked(&user.password_hash),
            id: user.id,
            username: user.username,
            role: user.role,
            display_name: user.display_name,
            title: user.title,
        }
    }
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<(CookieJar, Json<MeResponse>)> {
    let username = req.username.trim();
    let user = users::find_by_username(&state.pool, username)
        .await?
        .filter(|u| u.disabled == 0);

    // R3-F-6：用户不存在 / 已禁用时也跑一次等价 Argon2 校验，抹平用户名枚举时序。
    let hash = user
        .as_ref()
        .map(|u| u.password_hash.clone())
        .unwrap_or_else(|| password::DUMMY_TIMING_HASH.to_string());
    if !verify_password_async(req.password, hash).await {
        return Err(ApiError::InvalidCredentials);
    }
    let user = user.ok_or(ApiError::InvalidCredentials)?;

    issue_session(&state, jar, user).await
}

/// 签发会话并下发 Cookie（登录与引导完成共用）。
async fn issue_session(
    state: &AppState,
    jar: CookieJar,
    user: User,
) -> Result<(CookieJar, Json<MeResponse>)> {
    let token = session::generate_token();
    let token_hash = session::hash_token(&token);
    let now = now_unix();
    let snapshot = state.settings();
    let auth_settings = snapshot.auth.clone();
    // 内置 TLS 启用时 Cookie MUST 带 Secure（不依赖 `auth.cookie_secure` 手工开启）。
    let cookie_secure = auth_settings.cookie_secure || snapshot.tls.enabled;
    let ttl_seconds = (auth_settings.session_ttl_hours as i64).saturating_mul(3600);
    let expires_at = now + ttl_seconds;

    sessions::create(&state.pool, &token_hash, &user.id, now, expires_at).await?;

    let cookie = Cookie::build((COOKIE_NAME, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(cookie_secure)
        .max_age(Duration::seconds(ttl_seconds))
        .build();

    Ok((jar.add(cookie), Json(MeResponse::from(user))))
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode)> {
    if let Some(cookie) = jar.get(COOKIE_NAME) {
        let token_hash = session::hash_token(cookie.value());
        sessions::delete(&state.pool, &token_hash).await?;
    }
    Ok((jar.remove(Cookie::from(COOKIE_NAME)), StatusCode::NO_CONTENT))
}

pub async fn me(user: CurrentUser) -> Json<MeResponse> {
    Json(MeResponse::from(user.0))
}

// ---------------- 桌面自动登录 ----------------

#[derive(Deserialize)]
pub struct DesktopLoginRequest {
    pub token: String,
}

/// 桌面自动登录：用桌面壳交付的令牌换取 admin 会话。
///
/// - 令牌只在 desktop 模式生成，仅经 stdout 事件交付给壳，进程生命周期内可复用；
/// - 授权依据是令牌而非口令：admin 尚未设置口令（锁定态）时同样允许换取会话
///   （ADR 0017）。无口令只影响「口令登录」，不影响桌面免登录。
pub async fn desktop_login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<DesktopLoginRequest>,
) -> Result<(CookieJar, Json<MeResponse>)> {
    if !state.mode.is_desktop() {
        return Err(ApiError::DesktopLoginUnavailable);
    }
    let expected = state
        .desktop_login_hash
        .read()
        .expect("desktop login lock")
        .clone()
        .ok_or(ApiError::DesktopLoginUnavailable)?;
    if session::hash_token(req.token.trim()) != expected {
        return Err(ApiError::DesktopLoginUnavailable);
    }
    let user = users::find_by_username(&state.pool, ADMIN_USERNAME)
        .await?
        .filter(|user| user.disabled == 0 && user.archived_at.is_none())
        .ok_or(ApiError::DesktopLoginUnavailable)?;
    issue_session(&state, jar, user).await
}

#[derive(Deserialize)]
pub struct UpdateProfile {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

fn normalize(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub async fn update_me(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<UpdateProfile>,
) -> Result<Json<MeResponse>> {
    users::update_profile(
        &state.pool,
        &user.0.id,
        normalize(req.display_name).as_deref(),
        normalize(req.title).as_deref(),
        now_unix(),
    )
    .await?;
    let updated = users::find_by_id(&state.pool, &user.0.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(MeResponse::from(updated)))
}

#[derive(Deserialize)]
pub struct ChangePassword {
    /// 口令登录下的必填项；桌面令牌授权时可省略（见 `desktop_authorized`）。
    #[serde(default)]
    pub current_password: Option<String>,
    pub new_password: String,
    /// 桌面模式设置口令的授权凭据：与本进程桌面登录令牌一致即视为「本机主人」。
    #[serde(default)]
    pub desktop_token: Option<String>,
}

/// 桌面令牌是否可代表本机授权改口令。
///
/// 仅 `desktop` 模式 + 令牌哈希与本进程一致 + 调用者是 admin 时成立。
/// 令牌持有者 = 本机壳 = OS 用户，已能换取完整 admin 会话，因此不构成提权（ADR 0017）。
fn desktop_authorized(state: &AppState, user: &User, token: Option<&str>) -> bool {
    let Some(token) = token.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    if !state.mode.is_desktop() || user.role != "admin" {
        return false;
    }
    let expected = state
        .desktop_login_hash
        .read()
        .expect("desktop login lock")
        .clone();
    matches!(expected, Some(hash) if session::hash_token(token) == hash)
}

pub async fn change_password(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<ChangePassword>,
) -> Result<StatusCode> {
    let authorized = desktop_authorized(&state, &user.0, req.desktop_token.as_deref());
    if !authorized {
        let Some(current) = req.current_password.filter(|value| !value.is_empty()) else {
            return Err(ApiError::InvalidRequest("当前口令不能为空".to_string()));
        };
        if !verify_password_async(current, user.0.password_hash.clone()).await {
            return Err(ApiError::InvalidCredentials);
        }
    }
    if req.new_password.chars().count() < 8 {
        return Err(ApiError::InvalidRequest("新口令至少 8 位".to_string()));
    }
    let hash = hash_password_async(req.new_password).await?;
    users::update_password(&state.pool, &user.0.id, &hash, now_unix()).await?;
    if authorized {
        tracing::info!(
            user = %user.0.username,
            channel = "desktop-token",
            "admin password set without current password"
        );
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct BootstrapStatus {
    /// 是否存在待用引导令牌（不泄露令牌的任何信息）。
    pub pending: bool,
}

/// 查询是否处于「待引导」状态（公开接口，供设置页自检）。
pub async fn bootstrap_status(State(state): State<AppState>) -> Result<Json<BootstrapStatus>> {
    let pending = state
        .bootstrap
        .read()
        .expect("bootstrap lock")
        .is_some();
    Ok(Json(BootstrapStatus { pending }))
}

#[derive(Deserialize)]
pub struct BootstrapRequest {
    pub token: String,
    pub new_password: String,
}

/// 引导完成：写入 admin 口令、作废令牌，并直接签发会话。
///
/// 重复消费、令牌不匹配、目标账号已可用时一律返回 `invalid_bootstrap`。
pub async fn bootstrap(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<BootstrapRequest>,
) -> Result<(CookieJar, Json<MeResponse>)> {
    if req.new_password.chars().count() < 8 {
        return Err(ApiError::InvalidRequest("新口令至少 8 位".to_string()));
    }

    let pending = state
        .bootstrap
        .read()
        .expect("bootstrap lock")
        .clone()
        .ok_or(ApiError::InvalidBootstrap)?;
    // 令牌只存 SHA-256：比较哈希即可，无需常数时间比较（无法从哈希反推令牌）。
    if session::hash_token(req.token.trim()) != pending.token_hash {
        return Err(ApiError::InvalidBootstrap);
    }
    let user = users::find_by_id(&state.pool, &pending.user_id)
        .await?
        .ok_or(ApiError::InvalidBootstrap)?;
    if !is_locked(&user.password_hash) {
        return Err(ApiError::InvalidBootstrap);
    }

    let hash = hash_password_async(req.new_password).await?;
    users::update_password(&state.pool, &user.id, &hash, now_unix()).await?;
    // 用掉即作废：同一令牌不可重放。
    *state.bootstrap.write().expect("bootstrap lock") = None;
    // 通知桌面壳：引导已完成，后续「打开控制台」应改用桌面自动登录地址。
    if state.mode.is_desktop() {
        crate::events::bootstrap_consumed();
    }

    let updated = users::find_by_id(&state.pool, &user.id)
        .await?
        .ok_or(ApiError::InvalidBootstrap)?;
    issue_session(&state, jar, updated).await
}
