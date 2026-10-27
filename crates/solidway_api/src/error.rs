use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// 统一 API 错误：映射到 HTTP 状态码与 `{ "error": { "code", "message" } }`。
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("forbidden")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// 带稳定机器码的输入错误（`422`），用于控制台按码翻译。
    #[error("{message}")]
    Coded {
        code: &'static str,
        message: String,
    },
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("invalid bootstrap token")]
    InvalidBootstrap,
    #[error("cannot delete self")]
    CannotDeleteSelf,
    #[error("cannot remove last admin")]
    LastAdmin,
    #[error("restart unsupported")]
    RestartUnsupported,
    #[error("desktop settings unsupported")]
    DesktopUnsupported,
    #[error("desktop login unavailable")]
    DesktopLoginUnavailable,
    #[error("internal error: {0}")]
    Internal(String),

    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl ApiError {
    pub fn internal(message: impl Into<String>) -> Self {
        ApiError::Internal(message.into())
    }

    /// 构造带稳定机器码的 `422` 输入错误。
    pub fn coded(code: &'static str, message: impl Into<String>) -> Self {
        ApiError::Coded {
            code,
            message: message.into(),
        }
    }

    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", self.to_string()),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden", self.to_string()),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found", self.to_string()),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict", self.to_string()),
            ApiError::InvalidRequest(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "invalid_request", self.to_string())
            }
            ApiError::Coded { code, message } => {
                (StatusCode::UNPROCESSABLE_ENTITY, code, message.clone())
            }
            ApiError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                self.to_string(),
            ),
            ApiError::InvalidBootstrap => (
                StatusCode::UNAUTHORIZED,
                "invalid_bootstrap",
                self.to_string(),
            ),
            ApiError::CannotDeleteSelf => (
                StatusCode::CONFLICT,
                "cannot_delete_self",
                self.to_string(),
            ),
            ApiError::LastAdmin => (StatusCode::CONFLICT, "last_admin", self.to_string()),
            ApiError::RestartUnsupported => (
                StatusCode::CONFLICT,
                "restart_unsupported",
                "当前运行模式不支持自动重启，请手动重启 solidway".to_string(),
            ),
            ApiError::DesktopUnsupported => (
                StatusCode::CONFLICT,
                "desktop_unsupported",
                "该设置仅在桌面应用中可用".to_string(),
            ),
            ApiError::DesktopLoginUnavailable => (
                StatusCode::UNAUTHORIZED,
                "desktop_login_unavailable",
                "桌面自动登录不可用：令牌无效，或 admin 尚未设置口令".to_string(),
            ),
            ApiError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "internal server error".to_string(),
            ),
            ApiError::Database(_) | ApiError::Migration(_) | ApiError::Io(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "internal server error".to_string())
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        if status.is_server_error() {
            tracing::error!(error = %self, "request handling failed");
        }
        let body = json!({ "error": { "code": code, "message": message } });
        (status, Json(body)).into_response()
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;
