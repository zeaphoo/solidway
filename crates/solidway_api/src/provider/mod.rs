pub mod presets;
pub mod template;

use axum::Json;

use crate::auth::middleware::CurrentUser;
use crate::error::Result;

/// `GET /api/providers` — 返回内置 provider 目录（featured 在前），并附带默认入站格式。
pub async fn list_presets(_user: CurrentUser) -> Result<Json<Vec<presets::ProviderPresetView>>> {
    Ok(Json(presets::all().iter().map(Into::into).collect()))
}
