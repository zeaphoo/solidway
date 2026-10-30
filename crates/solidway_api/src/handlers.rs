use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, UtcOffset};

use crate::apikey;
use crate::auth::middleware::CurrentUser;
use crate::auth::password;
use crate::crypto::Cipher;
use crate::db::api_keys::{self, ApiKeyRow, ApiKeyWithRoute};
use crate::db::provider_health;
use crate::db::provider_keys::{self, ProviderKeyRow};
use crate::db::routes::{self, Route};
use crate::db::usage::{self, HIST_BUCKETS};
use crate::db::users::{self, User};
use crate::desktop;
use crate::error::{ApiError, Result};
use crate::provider::{presets, template};
use crate::settings::{self, DetailMode};
use crate::state::{AppState, new_id, now_unix};
use solidway_core::{Family, ModelMapping, normalize_base_url};

fn aad_for(id: &str) -> String {
    format!("provider_key|{id}")
}

fn models_from_json(raw: &str) -> Vec<ModelMapping> {
    serde_json::from_str::<Vec<ModelMapping>>(raw).unwrap_or_default()
}

fn models_to_json(models: &[ModelMapping]) -> String {
    let mut seen = std::collections::HashSet::new();
    let cleaned: Vec<ModelMapping> = models
        .iter()
        .filter_map(|mapping| {
            let name = mapping.name.trim();
            if name.is_empty() || !seen.insert(name.to_string()) {
                return None;
            }
            let upstream = mapping.upstream.trim();
            Some(ModelMapping {
                name: name.to_string(),
                upstream: if upstream.is_empty() {
                    name.to_string()
                } else {
                    upstream.to_string()
                },
            })
        })
        .collect();
    serde_json::to_string(&cleaned).unwrap_or_else(|_| "[]".to_string())
}

/// 校验并去重入站 API 格式；未定义取值 → 400。
fn normalize_api_formats(raw: &[String]) -> Result<Vec<String>> {
    let mut formats: Vec<String> = Vec::new();
    for item in raw {
        let value = item.trim();
        if value.is_empty() {
            continue;
        }
        let Some(format) = solidway_core::ApiFormat::parse(value) else {
            return Err(ApiError::InvalidRequest(format!(
                "未知的 api_format: {value}"
            )));
        };
        let format = format.as_str().to_string();
        if !formats.contains(&format) {
            formats.push(format);
        }
    }
    Ok(formats)
}

const AUTH_SCHEMES: [&str; 6] = [
    "bearer",
    "x-api-key",
    "api-key",
    "google_service_account",
    "sigv4",
    "none",
];

const CREDENTIAL_KINDS: [&str; 3] = ["api_key", "google_service_account", "aws_sigv4"];

fn normalize_credential_kind(raw: Option<&str>) -> Result<String> {
    let kind = raw.unwrap_or("api_key").trim();
    if !CREDENTIAL_KINDS.contains(&kind) {
        return Err(ApiError::InvalidRequest(format!(
            "未知的 credential_kind: {kind}"
        )));
    }
    Ok(kind.to_string())
}

/// Service Account JSON 的结构性校验；私钥只做存在性检查，不做密码学解析。
fn validate_service_account(raw: &str) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| ApiError::InvalidRequest("service_account_json 不是合法 JSON".to_string()))?;
    let object = value.as_object().ok_or_else(|| {
        ApiError::InvalidRequest("service_account_json 必须是 JSON 对象".to_string())
    })?;
    if object.get("type").and_then(|v| v.as_str()) != Some("service_account") {
        return Err(ApiError::InvalidRequest(
            "service_account_json.type 必须是 service_account".to_string(),
        ));
    }
    for field in ["client_email", "private_key", "token_uri"] {
        let valid = object
            .get(field)
            .and_then(|v| v.as_str())
            .is_some_and(|v| !v.trim().is_empty());
        if !valid {
            return Err(ApiError::InvalidRequest(format!(
                "service_account_json 缺少 {field}"
            )));
        }
    }
    Ok(())
}

/// AWS 凭据的结构性校验：只检查字段存在与非空。
fn validate_aws_credentials(raw: &str) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|_| ApiError::InvalidRequest("AWS 凭据不是合法 JSON".to_string()))?;
    let object = value
        .as_object()
        .ok_or_else(|| ApiError::InvalidRequest("AWS 凭据必须是 JSON 对象".to_string()))?;
    for field in ["access_key_id", "secret_access_key"] {
        let valid = object
            .get(field)
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty());
        if !valid {
            return Err(ApiError::InvalidRequest(format!("AWS 凭据缺少 {field}")));
        }
    }
    Ok(())
}

/// 掩码：暴露 access key id 首尾各 4 个字符，便于人工核对。
fn aws_credentials_masked(raw: &str) -> String {
    let access_key = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("access_key_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    format!("aws:{}", Cipher::mask(&access_key))
}

/// 按凭据类型校验内容并生成展示用掩码。
fn validate_and_mask(kind: &str, raw: &str) -> Result<String> {
    match kind {
        "google_service_account" => {
            validate_service_account(raw)?;
            Ok(service_account_masked(raw))
        }
        "aws_sigv4" => {
            validate_aws_credentials(raw)?;
            Ok(aws_credentials_masked(raw))
        }
        _ => {
            validate_api_key(raw)?;
            Ok(Cipher::mask(raw))
        }
    }
}

/// 展示用 DTO：掩码按当前规则从密文重新计算，保证掩码格式变更对存量凭据立即生效。
fn provider_key_dto(state: &AppState, row: ProviderKeyRow) -> ProviderKeyDto {
    let remasked = state
        .cipher
        .decrypt(&row.credential_ciphertext, &aad_for(&row.id))
        .ok()
        .and_then(|raw| validate_and_mask(&row.credential_kind, &raw).ok());
    let mut dto = ProviderKeyDto::from(row);
    if let Some(masked) = remasked {
        dto.credential_masked = masked;
    }
    dto
}

fn service_account_masked(raw: &str) -> String {
    let email = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("client_email")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    format!("service_account:{email}")
}

/// 校验鉴权方式；未提供时取预设默认，未命中预设回退 Bearer。
fn normalize_auth_scheme(
    raw: Option<String>,
    preset: Option<&presets::ProviderPreset>,
) -> Result<String> {
    match raw {
        Some(value) => {
            let value = value.trim();
            if value.is_empty() {
                return Err(ApiError::InvalidRequest("auth_scheme 不能为空".to_string()));
            }
            if !AUTH_SCHEMES.contains(&value) {
                return Err(ApiError::InvalidRequest(format!(
                    "未知的 auth_scheme: {value}"
                )));
            }
            Ok(value.to_string())
        }
        None => {
            let scheme = preset
                .map(|preset| preset.auth_scheme.clone())
                .unwrap_or_else(|| "bearer".to_string());
            if !AUTH_SCHEMES.contains(&scheme.as_str()) {
                return Err(ApiError::InvalidRequest(format!(
                    "Provider 预设暂不支持该鉴权方式: {scheme}"
                )));
            }
            Ok(scheme)
        }
    }
}

/// 端点解析结果：写入路径唯一入口。
struct ResolvedEndpoint {
    base_url: String,
    api_formats: Vec<String>,
    auth_scheme: String,
    options: Option<serde_json::Value>,
}

fn collect_vars(
    explicit: Option<&serde_json::Value>,
    options: Option<&serde_json::Value>,
) -> std::collections::BTreeMap<String, String> {
    let source = explicit.or_else(|| options.and_then(|value| value.get("base_url_vars")));
    let mut vars = std::collections::BTreeMap::new();
    if let Some(object) = source.and_then(|value| value.as_object()) {
        for (name, value) in object {
            if let Some(text) = value.as_str() {
                vars.insert(name.clone(), text.trim().to_string());
            }
        }
    }
    vars
}

/// 归一化选项：剥离服务端自有键，并把端点变量写回 `base_url_vars`。
fn merge_options(
    user: Option<serde_json::Value>,
    vars: &std::collections::BTreeMap<String, String>,
) -> Option<serde_json::Value> {
    let mut merged = serde_json::Map::new();
    if let Some(serde_json::Value::Object(map)) = user {
        for (key, value) in map {
            merged.insert(key, value);
        }
    }
    // `path_template` / `path_params` 由服务端按预设写入，客户端不得指定。
    merged.remove("base_url_vars");
    merged.remove("path_template");
    merged.remove("path_params");
    merged.remove("path_actions");
    merged.remove("aws_service");
    merged.remove("anthropic_version");
    if !vars.is_empty() {
        let object = vars
            .iter()
            .map(|(name, value)| (name.clone(), serde_json::Value::String(value.clone())))
            .collect();
        merged.insert(
            "base_url_vars".to_string(),
            serde_json::Value::Object(object),
        );
    }
    if merged.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(merged))
    }
}

/// 应用路径模板：校验凭据级路径参数，并把 `path_template` / `path_params` 冻结进 options。
///
/// 预设条目与 `<key>-custom` 副本都沿用预设的路径模板（端点形态不因"复制为自定义"而改变）；
/// 未关联任何预设的自定义条目没有模板，按 `{base}{家族后缀}` 转发。
fn apply_path_template(
    preset: Option<&presets::ProviderPreset>,
    mut options: Option<serde_json::Value>,
    path_var_values: &std::collections::BTreeMap<String, String>,
) -> Result<Option<serde_json::Value>> {
    let mut map = match options.take() {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    map.remove("path_template");
    map.remove("path_params");

    let Some(preset) = preset else {
        return Ok(if map.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(map))
        });
    };

    let mut path_params = serde_json::Map::new();
    for var in &preset.path_variables {
        let value = path_var_values.get(&var.name).cloned().unwrap_or_default();
        if value.is_empty() {
            if var.required {
                return Err(ApiError::coded(
                    "missing_path_var",
                    format!("缺少路径参数 {}", var.name),
                ));
            }
            continue;
        }
        template::validate_value(&var.kind, &value).map_err(|reason| {
            ApiError::coded(
                "invalid_path_var",
                format!("路径参数 {} 取值非法：{reason}", var.name),
            )
        })?;
        path_params.insert(var.name.clone(), serde_json::Value::String(value));
    }

    if let Some(template) = preset.path_template.clone() {
        // 模板占位符必须全部有来源：path_params、base_url_vars 或请求级 model/action/endpoint。
        map.insert(
            "path_template".to_string(),
            serde_json::Value::String(template),
        );
    }
    if let Some(actions) = &preset.path_actions {
        map.insert(
            "path_actions".to_string(),
            serde_json::to_value(actions).unwrap_or(serde_json::Value::Null),
        );
    }
    if let Some(service) = &preset.aws_service {
        map.insert(
            "aws_service".to_string(),
            serde_json::Value::String(service.clone()),
        );
    }
    if let Some(version) = &preset.anthropic_version {
        map.insert(
            "anthropic_version".to_string(),
            serde_json::Value::String(version.clone()),
        );
    }
    if !path_params.is_empty() {
        map.insert(
            "path_params".to_string(),
            serde_json::Value::Object(path_params),
        );
    }
    Ok(if map.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(map))
    })
}

/// 解析端点：预设条目按模板展开并锁定其余字段；自定义条目原样接受。
fn resolve_endpoint(
    provider: &str,
    base_url: Option<&str>,
    api_formats: Option<&[String]>,
    auth_scheme: Option<&str>,
    options: Option<serde_json::Value>,
    explicit_vars: Option<&serde_json::Value>,
    explicit_path_vars: Option<&serde_json::Value>,
) -> Result<ResolvedEndpoint> {
    let vars = collect_vars(explicit_vars, options.as_ref());
    let path_var_values = collect_vars(explicit_path_vars, None);
    let base = presets::preset_of(provider);
    let merged_options = merge_options(options, &vars);

    if let Some(preset) = presets::find(provider) {
        if let Some(url) = base_url.map(str::trim).filter(|url| !url.is_empty()) {
            return Err(ApiError::coded(
                "preset_field_immutable",
                format!(
                    "{provider} 的 base_url 由预设模板展开，不可直接指定（当前 {url}）；请填空完整端点请使用「复制为自定义」"
                ),
            ));
        }
        if api_formats.is_some() {
            return Err(ApiError::coded(
                "preset_field_immutable",
                format!("{provider} 的 api_formats 由预设决定，不可覆盖"),
            ));
        }
        if auth_scheme.filter(|value| !value.trim().is_empty()).is_some() {
            return Err(ApiError::coded(
                "preset_field_immutable",
                format!("{provider} 的 auth_scheme 由预设决定，不可覆盖"),
            ));
        }
        for var in &preset.base_url_variables {
            let value = vars.get(&var.name).cloned().unwrap_or_default();
            if value.is_empty() {
                if var.required {
                    return Err(ApiError::coded(
                        "missing_base_url_var",
                        format!("缺少端点变量 {}", var.name),
                    ));
                }
                continue;
            }
            template::validate_value(&var.kind, &value).map_err(|reason| {
                ApiError::coded(
                    "invalid_base_url_var",
                    format!("端点变量 {} 取值非法：{reason}", var.name),
                )
            })?;
        }
        let expanded = template::expand(&preset.base_url_template, &vars)?;
        validate_base_url(&expanded)?;

        let options = apply_path_template(Some(preset), merged_options, &path_var_values)?;

        return Ok(ResolvedEndpoint {
            base_url: template::trim_trailing_slash(&expanded),
            api_formats: default_api_formats(Some(preset)),
            auth_scheme: preset.auth_scheme.clone(),
            options,
        });
    }

    let url = base_url
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| {
            ApiError::coded(
                "base_url_required",
                format!("{provider} 不在内置目录中，需要提供 base_url"),
            )
        })?;
    validate_base_url(url)?;
    let api_formats = match api_formats {
        Some(list) => normalize_api_formats(list)?,
        None => default_api_formats(base),
    };
    let auth_scheme = match auth_scheme.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => normalize_auth_scheme(Some(value.to_string()), None)?,
        None => base
            .map(|preset| preset.auth_scheme.clone())
            .filter(|scheme| scheme != "google_service_account")
            .unwrap_or_else(|| "bearer".to_string()),
    };
    Ok(ResolvedEndpoint {
        base_url: template::trim_trailing_slash(url),
        api_formats,
        auth_scheme,
        options: apply_path_template(base, merged_options, &path_var_values)?,
    })
}

/// 凭据内容是否是 AWS 凭据 JSON（access key + secret）。
fn looks_like_aws_credentials(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .is_some_and(|object| {
            ["access_key_id", "secret_access_key"].iter().all(|field| {
                object
                    .get(*field)
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| !value.trim().is_empty())
            })
        })
}

/// 凭据内容是否是可用的 Service Account JSON。
fn looks_like_service_account(raw: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(|kind| kind.as_str())
                .map(|kind| kind == "service_account")
        })
        .unwrap_or(false)
}

/// Service Account 仅允许用于 Google 系端点。
fn ensure_service_account_allowed(provider: &str, base_url: &str) -> Result<()> {
    let preset_sa = presets::preset_of(provider)
        .map(|preset| preset.auth_scheme == "google_service_account")
        .unwrap_or(false);
    if preset_sa || presets::is_google_endpoint(base_url) {
        Ok(())
    } else {
        Err(ApiError::coded(
            "service_account_requires_google_preset",
            format!("Service Account 仅支持 Google 端点；当前端点 {base_url} 不匹配"),
        ))
    }
}

fn api_formats_to_json(formats: &[String]) -> String {
    serde_json::to_string(formats).unwrap_or_else(|_| "[]".to_string())
}

fn api_formats_from_json(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

/// 新 Provider 的默认入站格式：预设默认，未命中预设时按 OpenAI 兼容处理。
fn default_api_formats(preset: Option<&presets::ProviderPreset>) -> Vec<String> {
    match preset {
        Some(preset) => presets::default_api_formats(preset)
            .into_iter()
            .map(|format| format.as_str().to_string())
            .collect(),
        None => vec![
            solidway_core::ApiFormat::ChatCompletions
                .as_str()
                .to_string(),
        ],
    }
}

/// 校验上游 api_key：非空，且仅含可打印 ASCII 与空格（否则无法作为 HTTP header 值转发）。
fn validate_api_key(api_key: &str) -> Result<()> {
    if api_key.trim().is_empty() {
        return Err(ApiError::InvalidRequest("api_key 不能为空".to_string()));
    }
    if !api_key.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        return Err(ApiError::InvalidRequest(
            "api_key 含控制字符或非 ASCII 字符，无法作为 HTTP header 值转发（仅允许可打印 ASCII 与空格）"
                .to_string(),
        ));
    }
    Ok(())
}

/// 校验 base_url：绝对 http(s) URL、含 host、不含 query / fragment。
/// path 可以是空、`/v1` 或其它兼容前缀（如 `/openai`、`/api/paas/v4`），
/// 转发时只会在其后拼接 `/chat/completions`、`/responses` 等端点路径。
fn validate_base_url(url: &str) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(ApiError::InvalidRequest("base_url 不能为空".to_string()));
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(ApiError::InvalidRequest(format!(
            "base_url 必须是绝对 URL: {trimmed}"
        )));
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(ApiError::InvalidRequest(format!(
            "base_url 仅支持 http / https: {trimmed}"
        )));
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    if host.is_empty() {
        return Err(ApiError::InvalidRequest(format!(
            "base_url 缺少 host: {trimmed}"
        )));
    }
    if trimmed.contains(['?', '#']) {
        return Err(ApiError::InvalidRequest(format!(
            "base_url 不能包含 query 或 fragment: {trimmed}"
        )));
    }
    Ok(())
}

fn map_conflict(err: ApiError, message: &str) -> ApiError {
    if let ApiError::Database(sqlx::Error::Database(db)) = &err {
        if db.is_unique_violation() {
            return ApiError::Conflict(message.to_string());
        }
    }
    err
}

/// 校验路由存在且属于当前用户；admin 不例外。允许读取归档路由。
async fn ensure_owned_route(state: &AppState, user: &CurrentUser, id: &str) -> Result<Route> {
    routes::get_by_owner(&state.pool, id, &user.0.id)
        .await?
        .ok_or(ApiError::NotFound)
}

/// 校验路由存在、属于当前用户且未归档（用于写操作 / 新增子资源）。
async fn ensure_active_owned_route(
    state: &AppState,
    user: &CurrentUser,
    id: &str,
) -> Result<Route> {
    let route = ensure_owned_route(state, user, id).await?;
    if route.is_archived() {
        return Err(ApiError::Conflict("路由已归档，请先恢复".to_string()));
    }
    Ok(route)
}

fn normalize_status(status: Option<String>) -> Result<String> {
    match status.as_deref() {
        None | Some("active") => Ok("active".to_string()),
        Some("inactive") => Ok("inactive".to_string()),
        Some(other) => Err(ApiError::InvalidRequest(format!("status 非法: {other}"))),
    }
}

// ---------------- routes ----------------

#[derive(Deserialize)]
pub struct CreateRoute {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateRoute {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Deserialize)]
pub struct ListRoutesQuery {
    #[serde(default)]
    pub include_archived: Option<bool>,
}

pub async fn list_routes(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<ListRoutesQuery>,
) -> Result<Json<Vec<Route>>> {
    let rows = if query.include_archived.unwrap_or(false) {
        routes::list_by_owner_including_archived(&state.pool, &user.0.id).await?
    } else {
        routes::list_by_owner(&state.pool, &user.0.id).await?
    };
    Ok(Json(rows))
}

pub async fn create_route(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateRoute>,
) -> Result<(StatusCode, Json<Route>)> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::InvalidRequest("name 不能为空".to_string()));
    }
    let now = now_unix();
    // 9 位数字空间较小：生成时做存在性检查，最多重试 10 次。
    let id = {
        let mut generated = None;
        for _ in 0..10 {
            let candidate = crate::state::new_route_id();
            if routes::get(&state.pool, &candidate).await?.is_none() {
                generated = Some(candidate);
                break;
            }
        }
        generated.ok_or_else(|| ApiError::internal("无法生成唯一的路由 ID"))?
    };
    let route = Route {
        id,
        owner_id: Some(user.0.id.clone()),
        name,
        description: req.description,
        enabled: 1,
        created_at: now,
        updated_at: now,
        selection_strategy: solidway_core::SelectionStrategy::RoundRobin
            .as_str()
            .to_string(),
        selection_config: "{}".to_string(),
        archived_at: None,
        archived_by: None,
    };
    routes::create(&state.pool, &route)
        .await
        .map_err(|e| map_conflict(e, "路由名称已存在"))?;
    state.sync_core_best_effort().await;
    state.refresh_usage_owners().await;
    Ok((StatusCode::CREATED, Json(route)))
}

pub async fn get_route(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<Route>> {
    ensure_owned_route(&state, &user, &id).await.map(Json)
}

pub async fn update_route(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateRoute>,
) -> Result<Json<Route>> {
    let mut route = ensure_active_owned_route(&state, &user, &id).await?;
    if let Some(name) = req.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(ApiError::InvalidRequest("name 不能为空".to_string()));
        }
        route.name = name;
    }
    if let Some(description) = req.description {
        route.description = Some(description);
    }
    if let Some(enabled) = req.enabled {
        route.enabled = if enabled { 1 } else { 0 };
    }
    route.updated_at = now_unix();
    routes::update(&state.pool, &route)
        .await
        .map_err(|e| map_conflict(e, "路由名称已存在"))?;
    state.sync_core_best_effort().await;
    Ok(Json(route))
}

pub async fn delete_route(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    archive_route_impl(&state, &user.0.id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 归档路由（不可恢复）：
///   * 写 Provider / API Key 墓碑并删除活行，销毁密钥材料；
///   * 保留 Route 行用于历史归属；
///   * 关闭 open 故障、释放名称、移出数据面。
async fn archive_route_impl(state: &AppState, owner_id: &str, id: &str) -> Result<()> {
    let route = routes::get_by_owner(&state.pool, id, owner_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if route.is_archived() {
        return Ok(());
    }

    let providers = provider_keys::list_by_route(&state.pool, id).await?;
    let keys = api_keys::list_by_route(&state.pool, id).await?;
    let now = now_unix();

    let mut tx = state.pool.begin().await?;
    for provider in &providers {
        crate::db::tombstone::upsert(
            &mut tx,
            "provider_key",
            &provider.id,
            Some(id),
            route.owner_id.as_deref(),
            None,
            Some(provider.name.as_deref().unwrap_or(&provider.provider)),
            Some(&provider.provider),
            None,
            now,
            Some(owner_id),
            "route_archive",
        )
        .await?;
    }
    for key in &keys {
        let label = key.name.clone().unwrap_or_else(|| key.key_prefix.clone());
        crate::db::tombstone::upsert(
            &mut tx,
            "api_key",
            &key.id,
            Some(id),
            route.owner_id.as_deref(),
            None,
            Some(&label),
            None,
            Some(&key.key_prefix),
            now,
            Some(owner_id),
            "route_archive",
        )
        .await?;
    }
    // 删除活行：密文、哈希随行销毁；provider_health 由 FK CASCADE 清理。
    sqlx::query("DELETE FROM provider_keys WHERE route_id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM api_keys WHERE route_id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE routes SET archived_at = ?1, archived_by = ?2, updated_at = ?1
          WHERE id = ?3 AND owner_id = ?4 AND archived_at IS NULL",
    )
    .bind(now)
    .bind(owner_id)
    .bind(id)
    .bind(owner_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    // 归档后不再服务，故障历史保留但 open 必须收尾。
    let closed =
        crate::db::incidents::close_open_for_route(&state.pool, id, now, "route_archived").await?;
    if !closed.is_empty() {
        crate::alerts::notify_closed(state, &closed).await;
    }
    state.sync_core_best_effort().await;
    state.refresh_usage_owners().await;
    Ok(())
}

pub async fn archive_route(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    archive_route_impl(&state, &user.0.id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct TransferRoute {
    pub owner_id: String,
}

/// admin 转移路由归属；只改 owner，不追溯历史用量。
pub async fn transfer_route(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<TransferRoute>,
) -> Result<Json<Route>> {
    require_admin(&user)?;
    let route = routes::get(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if route.is_archived() {
        return Err(ApiError::Conflict("归档路由不可转移".to_string()));
    }
    let target = users::find_active(&state.pool, &req.owner_id)
        .await?
        .ok_or_else(|| ApiError::InvalidRequest("目标用户不存在或不可用".to_string()))?;
    if target.id == route.owner_id.clone().unwrap_or_default() {
        return Ok(Json(route));
    }
    let conflict: Option<String> = sqlx::query_scalar(
        "SELECT id FROM routes
          WHERE owner_id = ?1 AND name = ?2 AND archived_at IS NULL AND id != ?3
          LIMIT 1",
    )
    .bind(&target.id)
    .bind(&route.name)
    .bind(&route.id)
    .fetch_optional(&state.pool)
    .await?;
    if conflict.is_some() {
        return Err(ApiError::Conflict(format!(
            "目标用户已有同名活跃路由：{}",
            route.name
        )));
    }
    routes::transfer(&state.pool, &route.id, &target.id, now_unix()).await?;
    state.refresh_usage_owners().await;
    let route = routes::get(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(route))
}

// ---------------- provider keys ----------------

#[derive(Serialize)]
pub struct ProviderKeyDto {
    pub id: String,
    pub route_id: String,
    pub provider: String,
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub api_formats: Vec<String>,
    pub auth_scheme: String,
    pub credential_kind: String,
    pub credential_masked: String,
    /// 该凭据是否可被探针探测（Service Account / 空模型清单 / 仅 native 为 false）。
    pub probeable: bool,
    pub options: Option<serde_json::Value>,
    pub models: Vec<ModelMapping>,
    /// 选号档位：`0` = 不参与选号；档位高者先试。
    pub priority: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// 创建时的模型清单自动拉取结果；仅创建响应携带（列表 / 更新响应不带）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autodiscover: Option<AutodiscoverResult>,
}

/// 创建时模型清单自动拉取的结果（`autodiscover`）。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct AutodiscoverResult {
    pub status: AutodiscoverStatus,
    /// 仅 `failed` 时出现，给出可读原因。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 仅 `failed` 且需要界面按码翻译时出现（与 discover 端点同一套机器码）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutodiscoverStatus {
    Ok,
    Failed,
    Skipped,
}

impl From<ProviderKeyRow> for ProviderKeyDto {
    fn from(row: ProviderKeyRow) -> Self {
        let probeable =
            is_probeable(&row.credential_kind, &row.api_formats, &row.models);
        Self {
            id: row.id,
            route_id: row.route_id,
            provider: row.provider,
            name: row.name,
            base_url: row.base_url,
            api_formats: api_formats_from_json(&row.api_formats),
            auth_scheme: row.auth_scheme,
            credential_kind: row.credential_kind,
            credential_masked: row.credential_masked,
            probeable,
            options: row.options.and_then(|s| serde_json::from_str(&s).ok()),
            models: models_from_json(&row.models),
            priority: row.priority,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
            autodiscover: None,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateProviderKey {
    pub provider: String,
    #[serde(default)]
    pub api_key: Option<String>,
    /// api_key（默认）或 google_service_account。
    #[serde(default)]
    pub credential_kind: Option<String>,
    /// google_service_account 凭据的 JSON 原文。
    #[serde(default)]
    pub service_account_json: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    /// 端点模板变量取值（也可放在 `options.base_url_vars`）。
    #[serde(default)]
    pub base_url_vars: Option<serde_json::Value>,
    /// 路径模板参数取值（如 `project` / `location` / `deployment` / `api_version`）。
    #[serde(default)]
    pub path_vars: Option<serde_json::Value>,
    #[serde(default)]
    pub api_formats: Option<Vec<String>>,
    #[serde(default)]
    pub auth_scheme: Option<String>,
    #[serde(default)]
    pub options: Option<serde_json::Value>,
    #[serde(default)]
    pub models: Option<Vec<ModelMapping>>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateProviderKey {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub credential_kind: Option<String>,
    #[serde(default)]
    pub service_account_json: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    /// 端点模板变量取值（也可放在 `options.base_url_vars`）。
    #[serde(default)]
    pub base_url_vars: Option<serde_json::Value>,
    /// 路径模板参数取值（如 `project` / `location` / `deployment` / `api_version`）。
    #[serde(default)]
    pub path_vars: Option<serde_json::Value>,
    #[serde(default)]
    pub api_formats: Option<Vec<String>>,
    #[serde(default)]
    pub auth_scheme: Option<String>,
    #[serde(default)]
    pub options: Option<serde_json::Value>,
    #[serde(default)]
    pub models: Option<Vec<ModelMapping>>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn list_provider_keys(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(route_id): Path<String>,
) -> Result<Json<Vec<ProviderKeyDto>>> {
    ensure_owned_route(&state, &user, &route_id).await?;
    let rows = provider_keys::list_by_route(&state.pool, &route_id).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| provider_key_dto(&state, row))
            .collect(),
    ))
}

pub async fn create_provider_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(route_id): Path<String>,
    Json(req): Json<CreateProviderKey>,
) -> Result<(StatusCode, Json<ProviderKeyDto>)> {
    ensure_active_owned_route(&state, &user, &route_id).await?;
    let provider = req.provider.trim().to_string();
    if provider.is_empty() {
        return Err(ApiError::InvalidRequest("provider 不能为空".to_string()));
    }
    // 凭据内容判定类型：SA JSON 优先，其次 API Key。
    let (credential_kind, credential) = match req
        .service_account_json
        .as_deref()
        .filter(|raw| !raw.trim().is_empty())
    {
        Some(raw) => ("google_service_account".to_string(), raw.to_string()),
        None => {
            let raw = req
                .api_key
                .as_deref()
                .filter(|raw| !raw.trim().is_empty())
                .ok_or_else(|| {
                    ApiError::InvalidRequest("api_key 或 service_account_json 不能为空".to_string())
                })?;
            let kind = if looks_like_service_account(raw) {
                "google_service_account"
            } else if looks_like_aws_credentials(raw) {
                "aws_sigv4"
            } else {
                "api_key"
            };
            (kind.to_string(), raw.to_string())
        }
    };
    if let Some(hint) = req.credential_kind.as_deref() {
        let hint = normalize_credential_kind(Some(hint))?;
        if hint != credential_kind {
            return Err(ApiError::coded(
                "credential_kind_mismatch",
                format!("提交的 credential_kind（{hint}）与凭据内容（{credential_kind}）不一致"),
            ));
        }
    }
    let credential_masked = validate_and_mask(&credential_kind, &credential)?;

    let mut endpoint = resolve_endpoint(
        &provider,
        req.base_url.as_deref(),
        req.api_formats.as_deref(),
        req.auth_scheme.as_deref(),
        req.options.clone(),
        req.base_url_vars.as_ref(),
        req.path_vars.as_ref(),
    )?;
    // 凭据类型隐含的鉴权方式：显式冲突或预设锁定不匹配时报错，未提供时补齐。
    let required_auth = match credential_kind.as_str() {
        "google_service_account" => Some("google_service_account"),
        "aws_sigv4" => Some("sigv4"),
        _ => None,
    };
    if credential_kind == "google_service_account" {
        ensure_service_account_allowed(&provider, &endpoint.base_url)?;
    }
    match required_auth {
        Some(required) => {
            let explicit = req
                .auth_scheme
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty());
            if endpoint.auth_scheme != required && (explicit || presets::is_locked(&provider)) {
                return Err(ApiError::coded(
                    "incompatible_auth_scheme",
                    format!("{credential_kind} 凭据要求 auth_scheme = {required}"),
                ));
            }
            endpoint.auth_scheme = required.to_string();
        }
        None => {
            if matches!(endpoint.auth_scheme.as_str(), "google_service_account" | "sigv4") {
                return Err(ApiError::coded(
                    "incompatible_auth_scheme",
                    "该鉴权方式需要匹配的凭据类型（Service Account / AWS 凭据）",
                ));
            }
        }
    }
    let ResolvedEndpoint {
        base_url,
        api_formats,
        auth_scheme,
        options,
    } = endpoint;

    let id = new_id();
    let aad = aad_for(&id);
    let credential_ciphertext = state.cipher.encrypt(&credential, &aad)?;
    let credential_hash = state.cipher.fingerprint(&credential)?;
    let now = now_unix();

    if let Some(existing) =
        provider_keys::find_duplicate(&state.pool, &route_id, &base_url, &credential_hash, None)
            .await?
    {
        tracing::warn!(
            existing = %existing,
            route = %route_id,
            "duplicate provider credential (same base_url + api_key) in this route; coexisting, rotated independently"
        );
    }

    let models = models_to_json(&req.models.unwrap_or_default());
    // 归一化后的空清单才触发自动拉取：请求缺省、`[]`、只有空 `name` 行都算空。
    let models_empty = models_from_json(&models).is_empty();
    let mut row = ProviderKeyRow {
        id,
        route_id: route_id.clone(),
        provider,
        name: req.name,
        base_url: Some(base_url),
        api_formats: api_formats_to_json(&api_formats),
        auth_scheme,
        credential_kind,
        credential_ciphertext,
        credential_masked,
        credential_hash,
        options: options
            .map(|value| serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())),
        models,
        priority: normalize_priority(req.priority)?,
        status: normalize_status(req.status)?,
        created_at: now,
        updated_at: now,
    };

    // 先落库（创建必成），再自动拉取、成功回写；`sync_core` 只在最终态发一次。
    provider_keys::insert(&state.pool, &row).await?;
    let mut autodiscover = AutodiscoverResult {
        status: AutodiscoverStatus::Skipped,
        error: None,
        error_code: None,
    };
    if models_empty && row.credential_kind == "api_key" {
        // 自动拉取用**落库的** api_formats（与 discover 端点的请求草稿值相区分）。
        let family = discover_family(&api_formats_from_json(&row.api_formats));
        let outcome = discover_models_with(
            family,
            row.base_url.as_deref().unwrap_or_default(),
            &row.auth_scheme,
            &credential,
            std::time::Duration::from_secs(5),
        )
        .await;
        let mappings: Vec<ModelMapping> = outcome
            .models
            .iter()
            .map(|name| ModelMapping {
                name: name.clone(),
                upstream: name.clone(),
            })
            .collect();
        // `models_to_json` 负责 trim / 去重 / 丢弃空名，写入 0 条即失败。
        let discovered = models_to_json(&mappings);
        if models_from_json(&discovered).is_empty() {
            autodiscover = AutodiscoverResult {
                status: AutodiscoverStatus::Failed,
                error: Some(outcome.error.unwrap_or_else(|| "上游未返回模型".to_string())),
                error_code: outcome.error_code,
            };
        } else {
            let mut persisted = row.clone();
            persisted.models = discovered.clone();
            match provider_keys::update(&state.pool, &persisted).await {
                Ok(()) => {
                    row.models = discovered;
                    autodiscover = AutodiscoverResult {
                        status: AutodiscoverStatus::Ok,
                        error: None,
                        error_code: None,
                    };
                }
                Err(error) => {
                    tracing::error!(
                        %error,
                        provider_key = %row.id,
                        "failed to persist auto-discovered models; keeping the empty list"
                    );
                    autodiscover = AutodiscoverResult {
                        status: AutodiscoverStatus::Failed,
                        error: Some("模型清单写入失败".to_string()),
                        error_code: None,
                    };
                }
            }
        }
    }

    state.sync_core_best_effort().await;
    let mut dto = provider_key_dto(&state, row);
    dto.autodiscover = Some(autodiscover);
    Ok((StatusCode::CREATED, Json(dto)))
}

pub async fn get_provider_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<ProviderKeyDto>> {
    let row = provider_keys::get(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    ensure_owned_route(&state, &user, &row.route_id).await?;
    Ok(Json(provider_key_dto(&state, row)))
}

pub async fn update_provider_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderKey>,
) -> Result<Json<ProviderKeyDto>> {
    let mut row = provider_keys::get(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    ensure_owned_route(&state, &user, &row.route_id).await?;

    if let Some(provider) = req.provider.clone() {
        let provider = provider.trim().to_string();
        if provider.is_empty() {
            return Err(ApiError::InvalidRequest("provider 不能为空".to_string()));
        }
        row.provider = provider;
    }
    let locked = presets::is_locked(&row.provider);

    if let Some(name) = req.name.clone() {
        row.name = Some(name);
    }

    if locked {
        // 预设条目：端点与格式由目录决定，只允许通过变量值改变展开结果。
        let stored_options = row
            .options
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
        let options = req.options.clone().or(stored_options);
        let endpoint = resolve_endpoint(
            &row.provider,
            req.base_url.as_deref(),
            req.api_formats.as_deref(),
            req.auth_scheme.as_deref(),
            options,
            req.base_url_vars.as_ref(),
            req.path_vars.as_ref(),
        )?;
        row.base_url = Some(endpoint.base_url);
        row.api_formats = api_formats_to_json(&endpoint.api_formats);
        row.auth_scheme = endpoint.auth_scheme;
        row.options = endpoint
            .options
            .map(|value| serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string()));
    } else {
        if let Some(base_url) = req.base_url.clone() {
            validate_base_url(&base_url)?;
            row.base_url = Some(template::trim_trailing_slash(&base_url));
        }
        if let Some(formats) = req.api_formats.clone() {
            row.api_formats = api_formats_to_json(&normalize_api_formats(&formats)?);
        }
        if let Some(scheme) = req.auth_scheme.clone() {
            row.auth_scheme = normalize_auth_scheme(Some(scheme), None)?;
        }
        if req.options.is_some() || req.base_url_vars.is_some() {
            let stored_options = row
                .options
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
            let stored_vars = stored_options
                .as_ref()
                .and_then(|value| value.get("base_url_vars"))
                .cloned();
            let vars_value = req.base_url_vars.clone().or(stored_vars);
            let vars = collect_vars(vars_value.as_ref(), None);
            let merged = merge_options(req.options.clone().or(stored_options), &vars);
            row.options = merged
                .map(|value| serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string()));
        }
    }
    if let Some(priority) = req.priority {
        row.priority = normalize_priority(Some(priority))?;
    }
    if req.status.is_some() {
        row.status = normalize_status(req.status)?;
    }
    if let Some(models) = req.models.clone() {
        row.models = models_to_json(&models);
    }

    // 凭据更新：以内容判定类型，请求里的 credential_kind 仅作提示。
    let new_credential: Option<String> = if let Some(raw) = req
        .service_account_json
        .as_deref()
        .filter(|raw| !raw.trim().is_empty())
    {
        Some(raw.to_string())
    } else {
        req.api_key
            .as_deref()
            .filter(|raw| !raw.trim().is_empty())
            .map(str::to_string)
    };
    if let Some(raw) = new_credential {
        let kind = if looks_like_service_account(&raw) {
            "google_service_account"
        } else if looks_like_aws_credentials(&raw) {
            "aws_sigv4"
        } else {
            "api_key"
        };
        if let Some(hint) = req.credential_kind.as_deref() {
            let hint = normalize_credential_kind(Some(hint))?;
            if hint != kind {
                return Err(ApiError::coded(
                    "credential_kind_mismatch",
                    format!("提交的 credential_kind（{hint}）与凭据内容（{kind}）不一致"),
                ));
            }
        }
        let aad = aad_for(&row.id);
        let masked = validate_and_mask(kind, &raw)?;
        row.credential_ciphertext = state.cipher.encrypt(&raw, &aad)?;
        row.credential_masked = masked;
        row.credential_hash = state.cipher.fingerprint(&raw)?;
        row.credential_kind = kind.to_string();
    }

    if row.credential_kind == "google_service_account" {
        ensure_service_account_allowed(
            &row.provider,
            row.base_url.as_deref().unwrap_or_default(),
        )?;
        row.auth_scheme = "google_service_account".to_string();
    } else if row.auth_scheme == "google_service_account" {
        return Err(ApiError::coded(
            "incompatible_auth_scheme",
            "google_service_account 鉴权需要以 Service Account JSON 作为凭据",
        ));
    }
    row.updated_at = now_unix();

    provider_keys::update(&state.pool, &row).await?;
    state.sync_core_best_effort().await;
    Ok(Json(provider_key_dto(&state, row)))
}

pub async fn delete_provider_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let row = provider_keys::get(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let route = ensure_active_owned_route(&state, &user, &row.route_id).await?;
    let now = now_unix();

    // 先写墓碑再物理删除：墓碑保留展示身份，但不含任何密钥材料。
    let mut tx = state.pool.begin().await?;
    crate::db::tombstone::upsert(
        &mut tx,
        "provider_key",
        &row.id,
        Some(&row.route_id),
        route.owner_id.as_deref(),
        None,
        Some(row.name.as_deref().unwrap_or(&row.provider)),
        Some(&row.provider),
        None,
        now,
        Some(&user.0.id),
        "provider_delete",
    )
    .await?;
    sqlx::query("DELETE FROM provider_keys WHERE id = ?1")
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    // provider_health 由既有 FK CASCADE 清理；故障历史保留但需收尾。
    let closed =
        crate::db::incidents::close_open(&state.pool, &row.id, now, "resource_deleted").await?;
    if !closed.is_empty() {
        crate::alerts::notify_closed(&state, &closed).await;
    }
    state.sync_core_best_effort().await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------- 路由选号策略 ----------------

#[derive(Serialize)]
pub struct ProviderSelectionView {
    pub provider_id: String,
    pub name: Option<String>,
    pub priority: i64,
    pub enabled: bool,
    pub health: String,
    pub ttfb_ms: Option<f64>,
    pub uptime_pct: Option<f64>,
    pub samples: u64,
    pub stale: bool,
    pub score: Option<f64>,
}

/// `GET /api/routes/{id}/selection-strategy` 的响应。
#[derive(Serialize)]
pub struct SelectionStrategyView {
    pub strategy: &'static str,
    pub config: crate::selection::SelectionOverrides,
    pub resolved: crate::selection::ResolvedSelection,
    pub providers: Vec<ProviderSelectionView>,
}

async fn selection_view(state: &AppState, route: &routes::Route) -> Result<SelectionStrategyView> {
    let (strategy, overrides) =
        crate::selection::from_row(&route.selection_strategy, &route.selection_config);
    let resolved = crate::selection::resolve(strategy, &overrides);
    let core = state.core.read().expect("core lock").clone();
    let snapshot = match core {
        Some(core) => core
            .selection_snapshot(&route.id, None)
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    Ok(SelectionStrategyView {
        strategy: strategy.as_str(),
        config: overrides,
        resolved,
        providers: snapshot
            .into_iter()
            .map(|p| ProviderSelectionView {
                provider_id: p.provider_id,
                name: p.name,
                priority: p.priority,
                enabled: p.enabled,
                health: p.health,
                ttfb_ms: p.ttfb_ms,
                uptime_pct: p.uptime_pct,
                samples: p.samples,
                stale: p.stale,
                score: p.score,
            })
            .collect(),
    })
}

pub async fn get_selection_strategy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<Json<SelectionStrategyView>> {
    let route = routes::get_by_owner(&state.pool, &id, &user.0.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(selection_view(&state, &route).await?))
}

#[derive(Deserialize)]
pub struct UpdateSelectionStrategy {
    pub strategy: String,
    #[serde(default)]
    pub config: crate::selection::SelectionOverrides,
}

pub async fn put_selection_strategy(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateSelectionStrategy>,
) -> Result<Json<SelectionStrategyView>> {
    let route = ensure_active_owned_route(&state, &user, &id).await?;
    let Some(strategy) = solidway_core::SelectionStrategy::parse(req.strategy.trim()) else {
        return Err(ApiError::InvalidRequest(format!(
            "strategy 非法: {}（应为 round_robin / uptime / latency / auto）",
            req.strategy
        )));
    };
    crate::selection::validate(&req.config)?;
    let config = crate::selection::to_json(&req.config);
    let updated = routes::update_selection(
        &state.pool,
        &route.id,
        &user.0.id,
        strategy.as_str(),
        &config,
        now_unix(),
    )
    .await?;
    if !updated {
        return Err(ApiError::NotFound);
    }
    state.sync_core_best_effort().await;
    let route = routes::get_by_owner(&state.pool, &id, &user.0.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(selection_view(&state, &route).await?))
}

// ---------------- API Keys ----------------

#[derive(Serialize)]
pub struct ApiKeyDto {
    pub id: String,
    pub route_id: String,
    pub route_name: Option<String>,
    pub name: Option<String>,
    pub key_prefix: String,
    pub key_masked: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<ApiKeyRow> for ApiKeyDto {
    fn from(row: ApiKeyRow) -> Self {
        Self {
            id: row.id,
            route_id: row.route_id,
            route_name: None,
            name: row.name,
            key_prefix: row.key_prefix,
            key_masked: row.key_masked,
            status: row.status,
            expires_at: row.expires_at,
            last_used_at: row.last_used_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

impl From<ApiKeyWithRoute> for ApiKeyDto {
    fn from(row: ApiKeyWithRoute) -> Self {
        Self {
            id: row.id,
            route_id: row.route_id,
            route_name: row.route_name,
            name: row.name,
            key_prefix: row.key_prefix,
            key_masked: row.key_masked,
            status: row.status,
            expires_at: row.expires_at,
            last_used_at: row.last_used_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// 创建响应：仅此一次附带明文。
#[derive(Serialize)]
pub struct CreatedApiKey {
    #[serde(flatten)]
    pub api_key: ApiKeyDto,
    pub key: String,
}

#[derive(Deserialize)]
pub struct CreateApiKey {
    #[serde(default)]
    pub name: Option<String>,
    /// Unix 秒；缺省或 null = 永不过期。
    #[serde(default)]
    pub expires_at: Option<i64>,
}

/// 区分“未提供”与 JSON `null`：未提供 = 不修改，null = 清空（永不过期）。
fn deserialize_optional_field<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Option<i64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<i64>::deserialize(deserializer)?))
}

#[derive(Deserialize)]
pub struct UpdateApiKey {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_field")]
    pub expires_at: Option<Option<i64>>,
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<ApiKeyDto>>> {
    let rows = api_keys::list_by_owner(&state.pool, &user.0.id).await?;
    Ok(Json(rows.into_iter().map(ApiKeyDto::from).collect()))
}

pub async fn list_route_api_keys(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(route_id): Path<String>,
) -> Result<Json<Vec<ApiKeyDto>>> {
    ensure_owned_route(&state, &user, &route_id).await?;
    let rows = api_keys::list_by_route(&state.pool, &route_id).await?;
    Ok(Json(rows.into_iter().map(ApiKeyDto::from).collect()))
}

pub async fn create_api_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(route_id): Path<String>,
    Json(req): Json<CreateApiKey>,
) -> Result<(StatusCode, Json<CreatedApiKey>)> {
    ensure_active_owned_route(&state, &user, &route_id).await?;
    let route_name = routes::get(&state.pool, &route_id).await?.map(|r| r.name);

    let now = now_unix();
    if let Some(expires_at) = req.expires_at {
        if expires_at <= now {
            return Err(ApiError::InvalidRequest(
                "expires_at 必须晚于当前时间".to_string(),
            ));
        }
    }
    let generated = apikey::generate();
    let row = ApiKeyRow {
        id: new_id(),
        route_id,
        name: req.name,
        key_prefix: generated.prefix,
        key_masked: generated.masked,
        key_hash: generated.hash,
        status: "active".to_string(),
        expires_at: req.expires_at,
        last_used_at: None,
        created_at: now,
        updated_at: now,
    };
    api_keys::insert(&state.pool, &row).await?;

    let mut api_key = ApiKeyDto::from(row);
    api_key.route_name = route_name;
    state.sync_core_best_effort().await;
    Ok((
        StatusCode::CREATED,
        Json(CreatedApiKey {
            api_key,
            key: generated.plaintext,
        }),
    ))
}

pub async fn update_api_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateApiKey>,
) -> Result<Json<ApiKeyDto>> {
    let mut row = api_keys::get(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    ensure_owned_route(&state, &user, &row.route_id).await?;
    if let Some(name) = req.name {
        row.name = Some(name);
    }
    if req.status.is_some() {
        row.status = normalize_status(req.status)?;
    }
    if let Some(expires_at) = req.expires_at {
        row.expires_at = expires_at;
    }
    row.updated_at = now_unix();
    api_keys::update(&state.pool, &row).await?;

    let route_name = routes::get(&state.pool, &row.route_id)
        .await?
        .map(|r| r.name);
    let mut dto = ApiKeyDto::from(row);
    dto.route_name = route_name;
    state.sync_core_best_effort().await;
    Ok(Json(dto))
}

pub async fn delete_api_key(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let row = api_keys::get(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let route = ensure_active_owned_route(&state, &user, &row.route_id).await?;
    let now = now_unix();
    let label = row.name.clone().unwrap_or_else(|| row.key_prefix.clone());

    let mut tx = state.pool.begin().await?;
    crate::db::tombstone::upsert(
        &mut tx,
        "api_key",
        &row.id,
        Some(&row.route_id),
        route.owner_id.as_deref(),
        None,
        Some(&label),
        None,
        Some(&row.key_prefix),
        now,
        Some(&user.0.id),
        "api_key_delete",
    )
    .await?;
    sqlx::query("DELETE FROM api_keys WHERE id = ?1")
        .bind(&row.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    state.sync_core_best_effort().await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------- Users (admin) ----------------

#[derive(Serialize)]
pub struct UserDto {
    pub id: String,
    pub username: String,
    pub role: String,
    pub disabled: i64,
    pub display_name: Option<String>,
    pub title: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived_at: Option<i64>,
    pub archived_by: Option<String>,
}

impl From<User> for UserDto {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            role: user.role,
            disabled: user.disabled,
            display_name: user.display_name,
            title: user.title,
            created_at: user.created_at,
            updated_at: user.updated_at,
            archived_at: user.archived_at,
            archived_by: user.archived_by,
        }
    }
}

fn require_admin(user: &CurrentUser) -> Result<()> {
    if user.0.role == "admin" {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// 选号优先级（档位）：合法范围 `1..=1000`，数字越大越优先。
///
/// 「暂时不参与选号」用 Provider 状态（停用）表达，不接受 `0`。
fn normalize_priority(value: Option<i64>) -> Result<i64> {
    let priority = value.unwrap_or(1);
    if !(1..=1000).contains(&priority) {
        return Err(ApiError::InvalidRequest(
            "priority 必须在 1-1000 之间（数字越大越优先；不参与选号请停用该 Provider）"
                .to_string(),
        ));
    }
    Ok(priority)
}

fn normalize_opt(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn parse_role(role: Option<String>) -> Result<String> {
    match role.as_deref() {
        None | Some("user") => Ok("user".to_string()),
        Some("admin") => Ok("admin".to_string()),
        Some(other) => Err(ApiError::InvalidRequest(format!("role 非法: {other}"))),
    }
}

#[derive(Deserialize)]
pub struct ListUsersQuery {
    #[serde(default)]
    pub include_archived: Option<bool>,
}

pub async fn list_users(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<ListUsersQuery>,
) -> Result<Json<Vec<UserDto>>> {
    require_admin(&user)?;
    let rows = if query.include_archived.unwrap_or(false) {
        users::list_including_archived(&state.pool).await?
    } else {
        users::list(&state.pool).await?
    };
    Ok(Json(rows.into_iter().map(UserDto::from).collect()))
}

#[derive(Deserialize)]
pub struct CreateUser {
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Serialize)]
pub struct CreatedUser {
    #[serde(flatten)]
    pub user: UserDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

pub async fn create_user(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<CreateUser>,
) -> Result<(StatusCode, Json<CreatedUser>)> {
    require_admin(&user)?;
    let username = req.username.trim().to_string();
    if username.is_empty() {
        return Err(ApiError::InvalidRequest("username 不能为空".to_string()));
    }
    let role = parse_role(req.role)?;
    let (password, generated) = match req.password {
        Some(p) if !p.trim().is_empty() => (p, None),
        _ => {
            let generated = password::generate_password(20);
            (generated.clone(), Some(generated))
        }
    };
    let hash = password::hash_password_async(password).await?;
    let now = now_unix();
    let record = User {
        id: new_id(),
        username,
        password_hash: hash,
        role,
        disabled: 0,
        display_name: normalize_opt(req.display_name),
        title: normalize_opt(req.title),
        created_at: now,
        updated_at: now,
        archived_at: None,
        archived_by: None,
    };
    match users::insert(&state.pool, &record).await {
        Ok(()) => Ok((
            StatusCode::CREATED,
            Json(CreatedUser {
                user: UserDto::from(record),
                password: generated,
            }),
        )),
        Err(err) => Err(map_conflict(err, "用户名已存在")),
    }
}

#[derive(Deserialize)]
pub struct UpdateUser {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

pub async fn update_user(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateUser>,
) -> Result<Json<UserDto>> {
    require_admin(&user)?;
    let mut record = users::find_by_id(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if record.is_archived() {
        return Err(ApiError::Conflict("用户已归档，请先恢复".to_string()));
    }
    if req.role.is_some() {
        record.role = parse_role(req.role)?;
    }
    if let Some(disabled) = req.disabled {
        record.disabled = if disabled { 1 } else { 0 };
    }
    if req.display_name.is_some() {
        record.display_name = normalize_opt(req.display_name);
    }
    if req.title.is_some() {
        record.title = normalize_opt(req.title);
    }

    let still_admin = record.role == "admin" && record.disabled == 0;
    if !still_admin && users::count_active_admins_excluding(&state.pool, &id).await? == 0 {
        return Err(ApiError::LastAdmin);
    }

    record.updated_at = now_unix();
    users::update_admin_fields(&state.pool, &record).await?;
    Ok(Json(UserDto::from(record)))
}

#[derive(Deserialize)]
pub struct ResetPassword {
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Serialize)]
pub struct ResetPasswordResult {
    pub password: String,
}

pub async fn reset_user_password(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<ResetPassword>,
) -> Result<Json<ResetPasswordResult>> {
    require_admin(&user)?;
    let target = users::find_by_id(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if target.is_archived() {
        return Err(ApiError::Conflict("用户已归档，请先恢复".to_string()));
    }
    let password = match req.password {
        Some(p) if !p.trim().is_empty() => p,
        _ => password::generate_password(20),
    };
    let hash = password::hash_password_async(password.clone()).await?;
    users::update_password(&state.pool, &id, &hash, now_unix()).await?;
    Ok(Json(ResetPasswordResult { password }))
}

async fn archive_user_impl(state: &AppState, actor_id: &str, target_id: &str) -> Result<()> {
    if actor_id == target_id {
        return Err(ApiError::CannotDeleteSelf);
    }
    let target = users::find_by_id(&state.pool, target_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if target.is_archived() {
        return Ok(());
    }
    if target.role == "admin"
        && users::count_active_admins_excluding(&state.pool, target_id).await? == 0
    {
        return Err(ApiError::LastAdmin);
    }
    if routes::count_active_by_owner(&state.pool, target_id).await? > 0 {
        return Err(ApiError::Conflict(
            "用户仍拥有未归档路由，请先转移或归档其路由".to_string(),
        ));
    }
    let now = now_unix();
    users::archive(&state.pool, target_id, actor_id, now).await?;
    crate::db::sessions::delete_for_user(&state.pool, target_id).await?;
    Ok(())
}

pub async fn delete_user(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    archive_user_impl(&state, &user.0.id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn archive_user(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    archive_user_impl(&state, &user.0.id, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct OffboardUser {
    /// 继任者；存在未归档路由时必填。
    #[serde(default)]
    pub transfer_to: Option<String>,
}

/// admin offboarding：事务内转移全部未归档路由 → 归档用户 → 删除会话。
pub async fn offboard_user(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(req): Json<OffboardUser>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    if id == user.0.id {
        return Err(ApiError::CannotDeleteSelf);
    }
    let target = users::find_by_id(&state.pool, &id)
        .await?
        .ok_or(ApiError::NotFound)?;
    if target.is_archived() {
        return Ok(StatusCode::NO_CONTENT);
    }
    if target.role == "admin" && users::count_active_admins_excluding(&state.pool, &id).await? == 0
    {
        return Err(ApiError::LastAdmin);
    }

    let active_routes = routes::count_active_by_owner(&state.pool, &id).await?;
    let successor = match req.transfer_to.as_deref().filter(|value| !value.is_empty()) {
        Some(target_id) if target_id == id => {
            return Err(ApiError::InvalidRequest("继任者不能是用户自己".to_string()));
        }
        Some(target_id) => Some(
            users::find_active(&state.pool, target_id)
                .await?
                .ok_or_else(|| ApiError::InvalidRequest("继任者不存在或不可用".to_string()))?,
        ),
        None if active_routes == 0 => None,
        None => {
            return Err(ApiError::Conflict(
                "用户仍拥有未归档路由，请指定继任者".to_string(),
            ));
        }
    };

    if let Some(successor) = &successor {
        let conflicts: Vec<(String,)> = sqlx::query_as(
            "SELECT r.name FROM routes r
              WHERE r.owner_id = ?1 AND r.archived_at IS NULL
                AND EXISTS (
                    SELECT 1 FROM routes t
                     WHERE t.owner_id = ?2
                       AND t.archived_at IS NULL
                       AND t.name = r.name
                       AND t.id != r.id
                )",
        )
        .bind(&id)
        .bind(&successor.id)
        .fetch_all(&state.pool)
        .await?;
        if !conflicts.is_empty() {
            let names = conflicts
                .into_iter()
                .map(|(name,)| name)
                .collect::<Vec<_>>()
                .join("、");
            return Err(ApiError::Conflict(format!(
                "继任者已有同名活跃路由：{names}"
            )));
        }
    }

    let now = now_unix();
    let mut tx = state.pool.begin().await?;
    if let Some(successor) = &successor {
        sqlx::query(
            "UPDATE routes SET owner_id = ?1, updated_at = ?2
              WHERE owner_id = ?3 AND archived_at IS NULL",
        )
        .bind(&successor.id)
        .bind(now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "UPDATE users
            SET archived_at = ?1, archived_by = ?2, disabled = 1, updated_at = ?1
          WHERE id = ?3 AND archived_at IS NULL",
    )
    .bind(now)
    .bind(&user.0.id)
    .bind(&id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    state.refresh_usage_owners().await;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------- System info ----------------

#[derive(Serialize)]
pub struct WebAssetsInfo {
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub files: usize,
}

#[derive(Serialize)]
pub struct SystemInfo {
    pub version: String,
    pub web_assets: WebAssetsInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_root: Option<String>,
}

pub async fn system_info(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<SystemInfo>> {
    require_admin(&user)?;
    let assets = state.web_assets.as_ref();
    Ok(Json(SystemInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        web_assets: WebAssetsInfo {
            source: assets.source,
            path: assets.path.clone(),
            files: assets.files,
        },
        data_root: Some(state.data_root.to_string_lossy().to_string()),
    }))
}

#[derive(Serialize)]
pub struct VersionInfo {
    pub version: String,
}

/// 当前版本：任意已认证角色可见（非 admin 看不到「系统」页，但需要知道版本）。
///
/// 只回版本号，不附带任何运行事实（那些仍归 `/api/system`、`/api/app/info`，仅 admin）。
pub async fn version(_user: CurrentUser) -> Json<VersionInfo> {
    Json(VersionInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

// ---------------- App info / restart ----------------

#[derive(Serialize)]
pub struct ListenView {
    pub effective: String,
    pub source: &'static str,
    pub restart_required: bool,
}

/// 只返回界面需要的信息：版本、模式、能否重启、监听事实。
#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub mode: &'static str,
    pub can_restart: bool,
    pub listen: ListenView,
}

/// 实际监听地址与数据库目标值是否一致（IPv4/IPv6 均按 `SocketAddr` 比较）。
/// 实际监听集合与配置集合是否一致（多地址时按集合比较）。
fn listen_matches(effective: &[String], network: &settings::NetworkSettings) -> bool {
    let mut expected: Vec<String> = network
        .bind_ips()
        .into_iter()
        .map(|ip| format!("{ip}:{}", network.port))
        .collect();
    let mut actual: Vec<String> = effective.to_vec();
    expected.sort();
    actual.sort();
    expected == actual
}

pub async fn app_info(State(state): State<AppState>, user: CurrentUser) -> Result<Json<AppInfo>> {
    require_admin(&user)?;
    let settings = state.settings();
    let listen = state.listen.read().expect("listen lock").clone();
    let (effective, source) = match listen {
        Some(info) => (info.effective, info.source),
        None => (Vec::new(), crate::state::ListenSource::Database),
    };
    let restart_required = matches!(source, crate::state::ListenSource::Database)
        && !listen_matches(&effective, &settings.network);
    let effective = if effective.is_empty() {
        "-".to_string()
    } else {
        effective.join(", ")
    };
    Ok(Json(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        mode: state.mode.as_str(),
        can_restart: state.mode.is_desktop(),
        listen: ListenView {
            effective,
            source: source.as_str(),
            restart_required,
        },
    }))
}

/// 请求托管方重启：响应写完后输出事件并退出。仅 desktop 模式可用。
pub async fn app_restart(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<(StatusCode, Json<serde_json::Value>)> {
    require_admin(&user)?;
    if !state.mode.is_desktop() {
        return Err(ApiError::RestartUnsupported);
    }
    crate::events::restart_requested("network-settings");
    // 与 SIGTERM / stdin EOF 共用统一关闭通道：由 main 停止接受新连接、
    // 等待在途请求并执行 flush + 快照，再统一退出。
    if !state.request_shutdown() {
        return Err(ApiError::internal("关闭通道尚未就绪"));
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "restarting": true })),
    ))
}

// ---------------- 桌面偏好（仅 desktop 模式） ----------------

/// `GET /api/app/desktop` 的响应。standalone 模式只回 `available = false`。
#[derive(Serialize)]
pub struct DesktopView {
    pub available: bool,
    pub prefs: Option<desktop::Prefs>,
    pub prefs_source: Option<desktop::PrefsSource>,
    pub status: Option<desktop::Status>,
}

/// 当前桌面偏好与最近一次回执。
///
/// 偏好与回执都由桌面壳写入，本接口只读：`prefs` 在壳落盘前仍是旧值，
/// 控制台据此等待 `status.apply_id` 匹配的回执。
pub async fn app_desktop_get(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<DesktopView>> {
    require_admin(&user)?;
    if !state.mode.is_desktop() {
        return Ok(Json(DesktopView {
            available: false,
            prefs: None,
            prefs_source: None,
            status: None,
        }));
    }
    let (prefs, source) = desktop::load_prefs(&state.data_root);
    Ok(Json(DesktopView {
        available: true,
        prefs: Some(prefs),
        prefs_source: Some(source),
        status: desktop::load_status(&state.data_root),
    }))
}

#[derive(Debug, Deserialize)]
pub struct DesktopPatch {
    pub autostart: bool,
    pub close_window_to_tray: bool,
}

/// 请求桌面壳应用偏好：只发 stdout 事件，**不写** `desktop.toml`。
///
/// 壳执行成功后落盘并写 `desktop.status.json`；控制台按 `apply_id` 等回执。
pub async fn app_desktop_put(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<DesktopPatch>,
) -> Result<(StatusCode, Json<serde_json::Value>)> {
    require_admin(&user)?;
    if !state.mode.is_desktop() {
        return Err(ApiError::DesktopUnsupported);
    }
    let apply_id = format!("{:08x}", rand::random::<u32>());
    crate::events::desktop_apply(&apply_id, req.autostart, req.close_window_to_tray);
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "apply_id": apply_id, "pending": true })),
    ))
}

// ---------------- 网络候选地址 ----------------

#[derive(Serialize)]
pub struct LocalAddress {
    pub ip: String,
    /// 网卡名（如 eth0 / en0 / 以太网），仅用于界面展示。
    pub interface: String,
    /// 是否为虚拟网卡（docker0 / br-* / veth / virbr* / tun* 等）。
    /// 界面默认收起，避免把一堆网桥地址铺在候选里。
    pub virtual_nic: bool,
}

#[derive(Serialize)]
pub struct NetworkAddresses {
    pub loopback: Vec<String>,
    pub wildcard: Vec<String>,
    pub local: Vec<LocalAddress>,
}

/// `GET /api/network/addresses`：界面"快捷选择"用的候选地址（仅 admin）。
///
/// `local` 枚举本机全部网卡地址（排除回环 / link-local / 未指定）；
/// 枚举不可用时回退为"默认出网地址"（UDP connect，不发包）。
pub async fn network_addresses(
    State(_state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<NetworkAddresses>> {
    require_admin(&user)?;
    let local = tokio::task::spawn_blocking(detect_local_addresses)
        .await
        .unwrap_or_default();
    Ok(Json(NetworkAddresses {
        loopback: vec!["127.0.0.1".to_string(), "::1".to_string()],
        wildcard: vec!["0.0.0.0".to_string(), "::".to_string()],
        local,
    }))
}

/// 枚举本机网卡地址：IPv4 在前，按网卡名 + IP 排序并去重。
fn detect_local_addresses() -> Vec<LocalAddress> {
    let mut out: Vec<LocalAddress> = Vec::new();
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for iface in interfaces {
            let ip = iface.ip();
            if ip.is_loopback() || ip.is_unspecified() {
                continue;
            }
            // link-local（169.254.* / fe80::）不能作为对外监听地址。
            let link_local = match ip {
                std::net::IpAddr::V4(v4) => v4.is_link_local(),
                std::net::IpAddr::V6(v6) => v6.is_unicast_link_local(),
            };
            if link_local {
                continue;
            }
            let ip = ip.to_string();
            if out.iter().any(|item| item.ip == ip) {
                continue;
            }
            out.push(LocalAddress {
                ip,
                virtual_nic: is_virtual_interface(&iface.name),
                interface: iface.name,
            });
        }
    }

    if out.is_empty() {
        // 兜底：默认出网地址（UDP connect 不产生流量）。
        if let Some(ip) = detect_primary_ip() {
            out.push(LocalAddress {
                ip,
                interface: String::new(),
                virtual_nic: false,
            });
        }
    }

    // 物理网卡在前 → IPv4 在前 → 按网卡名与 IP。
    out.sort_by(|a, b| {
        a.virtual_nic
            .cmp(&b.virtual_nic)
            .then_with(|| b.ip.contains('.').cmp(&a.ip.contains('.')))
            .then_with(|| a.interface.cmp(&b.interface))
            .then_with(|| a.ip.cmp(&b.ip))
    });
    out
}

/// 判断是否虚拟网卡（docker / 网桥 / 隧道等）：界面默认收起这些地址。
fn is_virtual_interface(name: &str) -> bool {
    const PREFIXES: [&str; 14] = [
        "br-",
        "veth",
        "docker",
        "virbr",
        "vmnet",
        "vboxnet",
        "tun",
        "tap",
        "wg",
        "zt",
        "tailscale",
        "utun",
        "cni",
        "flannel",
    ];
    let lower = name.to_ascii_lowercase();
    if lower == "lo" || lower.starts_with("lo0") {
        return true;
    }
    PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
        || lower.contains("-tun")
        || lower.contains("bridge")
}

/// 默认出网 IP（UDP connect 不产生流量）。
fn detect_primary_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    let ip = addr.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

// ---------------- 故障记录 ----------------

#[derive(Deserialize)]
pub struct IncidentQuery {
    #[serde(default)]
    pub route_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default)]
    pub limit: Option<i64>,
}

async fn incident_rows(
    state: &AppState,
    user: &CurrentUser,
    query: &IncidentQuery,
    owner_scope: bool,
) -> Result<Json<Vec<crate::db::incidents::IncidentRow>>> {
    let owner_route_ids = if owner_scope {
        Some(routes::list_ids_by_owner(&state.pool, &user.0.id, true).await?)
    } else {
        None
    };
    let state_filter = query
        .state
        .as_deref()
        .filter(|value| matches!(*value, "open" | "closed"));
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let filter = crate::db::incidents::IncidentFilter {
        owner_route_ids,
        route_id: query.route_id.as_deref(),
        provider_id: query.provider_id.as_deref(),
        state: state_filter,
        since: query.since,
    };
    Ok(Json(
        crate::db::incidents::list(&state.pool, &filter, limit).await?,
    ))
}

/// `GET /api/provider-incidents`：普通用户只看自己路由下的故障。
pub async fn list_incidents(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<IncidentQuery>,
) -> Result<Json<Vec<crate::db::incidents::IncidentRow>>> {
    incident_rows(&state, &user, &query, true).await
}

/// `GET /api/admin/provider-incidents`：仅 admin，全站。
pub async fn admin_list_incidents(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<IncidentQuery>,
) -> Result<Json<Vec<crate::db::incidents::IncidentRow>>> {
    require_admin(&user)?;
    incident_rows(&state, &user, &query, false).await
}

/// `POST /api/admin/provider-incidents/{id}/close`：按记录 id 关闭未解决故障。
pub async fn admin_close_incident(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    let closed = crate::db::incidents::close_one(&state.pool, &id, now_unix(), "manual").await?;
    if !closed {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/provider-incidents/stats`：各 Provider 近 24h 故障次数与累计时长。
///
/// admin 看全站，普通用户只看自己路由下的 Provider。
pub async fn incident_stats(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<crate::db::incidents::IncidentStat>>> {
    let now = now_unix();
    let owner_route_ids = if user.0.role == "admin" {
        None
    } else {
        Some(routes::list_ids_by_owner(&state.pool, &user.0.id, true).await?)
    };
    Ok(Json(
        crate::db::incidents::stats_since(
            &state.pool,
            now - 24 * 3600,
            now,
            owner_route_ids.as_deref(),
        )
        .await?,
    ))
}

// ---------------- TLS (admin) ----------------

/// `GET /api/tls/status`：证书域名、状态、到期、最后错误、端口映射提示。
pub async fn tls_status(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<crate::tls::TlsStatus>> {
    require_admin(&user)?;
    Ok(Json(crate::tls::status_for(&state).await?))
}

/// `POST /api/tls/retry`：立即再尝试一次签发 / 续期（异步受理）。
pub async fn tls_retry(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<(StatusCode, Json<serde_json::Value>)> {
    require_admin(&user)?;
    let settings = settings::load_tls(&state.pool).await?;
    if !settings.enabled {
        return Err(ApiError::InvalidRequest(
            "内置 HTTPS 未启用，无法重试签发".to_string(),
        ));
    }
    match state.tls() {
        Some(runtime) => {
            runtime.request_retry();
            Ok((
                StatusCode::ACCEPTED,
                Json(serde_json::json!({ "accepted": true })),
            ))
        }
        // 启用需要重启监听器：此时还没有运行中的 ACME 任务。
        None => Ok((
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "accepted": false, "restart_required": true })),
        )),
    }
}

// ---------------- Settings (admin) ----------------

pub async fn get_settings(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<settings::AllSettings>> {
    require_admin(&user)?;
    Ok(Json(settings::load_all(&state.pool).await?))
}

/// `GET /api/access`：对外接入地址，任意已认证角色可读。
pub async fn get_access(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> Result<Json<settings::AccessSettings>> {
    Ok(Json(settings::load_access(&state.pool).await?))
}

#[derive(Deserialize)]
pub struct UsageSettingsPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub detail_mode: Option<String>,
    #[serde(default)]
    pub detail_retention_hours: Option<u32>,
    #[serde(default)]
    pub detail_sample_rate: Option<f64>,
    #[serde(default)]
    pub timezone_offset_minutes: Option<i64>,
}

#[derive(Deserialize)]
pub struct GatewaySettingsPatch {
    #[serde(default)]
    pub cooldown_auth_secs: Option<u64>,
    #[serde(default)]
    pub cooldown_rate_limit_secs: Option<u64>,
    #[serde(default)]
    pub cooldown_unavailable_secs: Option<u64>,
    #[serde(default)]
    pub quota_cooldown_secs: Option<u64>,
    #[serde(default)]
    pub affinity_ttl_secs: Option<u64>,
}

#[derive(Deserialize)]
pub struct NetworkSettingsPatch {
    #[serde(default)]
    pub bind_addr: Option<String>,
    #[serde(default)]
    pub bind_addrs: Option<Vec<String>>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub connect_timeout_secs: Option<u64>,
    #[serde(default)]
    pub first_byte_timeout_secs: Option<u64>,
}

#[derive(Deserialize)]
pub struct AuthSettingsPatch {
    #[serde(default)]
    pub session_ttl_hours: Option<u64>,
}

#[derive(Deserialize)]
pub struct AccessSettingsPatch {
    #[serde(default)]
    pub public_origins: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct AlertSettingsPatch {
    #[serde(default)]
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub notify_recovery: Option<bool>,
    #[serde(default)]
    pub min_interval_minutes: Option<u64>,
}

#[derive(Deserialize)]
pub struct TlsSettingsPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub challenge: Option<String>,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub http_port: Option<u16>,
    #[serde(default)]
    pub https_port: Option<u16>,
    #[serde(default)]
    pub hsts: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateSettings {
    #[serde(default)]
    pub usage: Option<UsageSettingsPatch>,
    #[serde(default)]
    pub gateway: Option<GatewaySettingsPatch>,
    #[serde(default)]
    pub network: Option<NetworkSettingsPatch>,
    #[serde(default)]
    pub auth: Option<AuthSettingsPatch>,
    #[serde(default)]
    pub access: Option<AccessSettingsPatch>,
    #[serde(default)]
    pub tls: Option<TlsSettingsPatch>,
    #[serde(default)]
    pub alert: Option<AlertSettingsPatch>,
}

/// `PATCH /api/settings` 的响应：`AllSettings` 的字段保持扁平（前端读法不变），
/// 额外带 `restart_required`（监听相关设置改动后为 `true`）。
#[derive(Serialize)]
pub struct SettingsPatched {
    #[serde(flatten)]
    pub settings: settings::AllSettings,
    pub restart_required: bool,
}

pub async fn patch_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<UpdateSettings>,
) -> Result<Json<SettingsPatched>> {
    require_admin(&user)?;

    let tls_before = settings::load_tls(&state.pool).await?;
    let mut tls_changed = false;

    if let Some(patch) = req.usage {
        let mut usage = settings::load_usage(&state.pool).await?;
        if let Some(value) = patch.enabled {
            usage.enabled = value;
        }
        if let Some(value) = patch.detail_mode {
            usage.detail_mode = match value.as_str() {
                "off" => DetailMode::Off,
                "errors" => DetailMode::Errors,
                "all" => DetailMode::All,
                other => {
                    return Err(ApiError::InvalidRequest(format!(
                        "detail_mode 非法: {other}"
                    )));
                }
            };
        }
        if let Some(value) = patch.detail_retention_hours {
            usage.detail_retention_hours = value;
        }
        if let Some(value) = patch.detail_sample_rate {
            usage.detail_sample_rate = value;
        }
        if patch.timezone_offset_minutes.is_some() {
            return Err(ApiError::InvalidRequest(
                "统计时区由服务端决定，不可修改（改偏移会让历史桶与新桶边界错位）".to_string(),
            ));
        }
        settings::save_usage(&state.pool, &usage, &user.0.id, now_unix()).await?;
    }

    if let Some(patch) = req.gateway {
        let mut gateway = settings::load_gateway(&state.pool).await?;
        if let Some(value) = patch.cooldown_auth_secs {
            gateway.cooldown_auth_secs = value;
        }
        if let Some(value) = patch.cooldown_rate_limit_secs {
            gateway.cooldown_rate_limit_secs = value;
        }
        if let Some(value) = patch.cooldown_unavailable_secs {
            gateway.cooldown_unavailable_secs = value;
        }
        if let Some(value) = patch.quota_cooldown_secs {
            gateway.quota_cooldown_secs = value;
        }
        if let Some(value) = patch.affinity_ttl_secs {
            gateway.affinity_ttl_secs = value;
        }
        settings::save_gateway(&state.pool, &gateway, &user.0.id, now_unix()).await?;
    }

    if let Some(patch) = req.network {
        let mut network = settings::load_network(&state.pool).await?;
        // 列表优先；只传旧字段时视为单元素列表。
        if let Some(list) = patch.bind_addrs {
            network.bind_addrs = list;
        } else if let Some(value) = patch.bind_addr {
            network.bind_addrs = vec![value.trim().to_string()];
        }
        if let Some(value) = patch.port {
            network.port = value;
        }
        if let Some(value) = patch.connect_timeout_secs {
            network.connect_timeout_secs = value;
        }
        if let Some(value) = patch.first_byte_timeout_secs {
            network.first_byte_timeout_secs = value;
        }
        settings::save_network(&state.pool, &network, &user.0.id, now_unix()).await?;
    }

    if let Some(patch) = req.auth {
        let mut auth = settings::load_auth(&state.pool).await?;
        if let Some(value) = patch.session_ttl_hours {
            auth.session_ttl_hours = value;
        }
        // `cookie_secure` 刻意不通过 API 暴露，见 settings::AuthSettings 的说明。
        settings::save_auth(&state.pool, &auth, &user.0.id, now_unix()).await?;
    }

    if let Some(patch) = req.access {
        let mut access = settings::load_access(&state.pool).await?;
        if let Some(origins) = patch.public_origins {
            access.public_origins = origins
                .into_iter()
                .map(|url| url.trim().trim_end_matches('/').to_string())
                .filter(|url| !url.is_empty())
                .collect();
        }
        settings::save_access(&state.pool, &access, &user.0.id, now_unix()).await?;
    }

    if let Some(patch) = req.tls {
        let mut tls = settings::load_tls(&state.pool).await?;
        if let Some(value) = patch.enabled {
            tls.enabled = value;
        }
        if let Some(value) = patch.email {
            tls.email = value.trim().to_string();
        }
        if let Some(value) = patch.challenge {
            tls.challenge = match value.as_str() {
                "http-01" => settings::TlsChallenge::Http01,
                "tls-alpn-01" => settings::TlsChallenge::TlsAlpn01,
                other => {
                    return Err(ApiError::InvalidRequest(format!(
                        "challenge 非法: {other}（应为 http-01 / tls-alpn-01）"
                    )));
                }
            };
        }
        if let Some(value) = patch.directory {
            tls.directory = match value.as_str() {
                "production" => settings::AcmeDirectory::Production,
                "staging" => settings::AcmeDirectory::Staging,
                other => {
                    return Err(ApiError::InvalidRequest(format!(
                        "directory 非法: {other}（应为 production / staging）"
                    )));
                }
            };
        }
        if let Some(value) = patch.http_port {
            tls.http_port = value;
        }
        if let Some(value) = patch.https_port {
            tls.https_port = value;
        }
        if let Some(value) = patch.hsts {
            tls.hsts = value;
        }
        // 监听相关字段变化需要重启；续期不需要。
        tls_changed = tls.enabled != tls_before.enabled
            || tls.http_port != tls_before.http_port
            || tls.https_port != tls_before.https_port
            || tls.challenge != tls_before.challenge
            || tls.directory != tls_before.directory;
        settings::save_tls(&state.pool, &tls, &user.0.id, now_unix()).await?;
    }

    if let Some(patch) = req.alert {
        let mut alert = settings::load_alert(&state.pool).await?;
        if let Some(url) = patch.webhook_url {
            let trimmed = url.trim().to_string();
            alert.webhook_url = (!trimmed.is_empty()).then_some(trimmed);
        }
        if let Some(value) = patch.notify_recovery {
            alert.notify_recovery = value;
        }
        if let Some(value) = patch.min_interval_minutes {
            alert.min_interval_minutes = value;
        }
        settings::save_alert(&state.pool, &alert, &user.0.id, now_unix()).await?;
    }

    // 先刷新快照，让随后的 core 下发用到最新超时。
    state.reload_settings().await?;
    state.sync_core_best_effort().await;
    state.refresh_usage_settings().await;
    Ok(Json(SettingsPatched {
        settings: settings::load_all(&state.pool).await?,
        restart_required: tls_changed,
    }))
}

// ---------------- Model discovery ----------------

#[derive(Deserialize)]
pub struct DiscoverModels {
    pub provider: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    /// 引用某条已保存的 provider 凭据（编辑态 api_key 留空时使用）。
    #[serde(default)]
    pub provider_key_id: Option<String>,
    #[serde(default)]
    pub auth_scheme: Option<String>,
    /// 表单草稿的入站格式；缺省按 provider 预设推断，仍缺省视为 `chat_completions`。
    #[serde(default)]
    pub api_formats: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct DiscoverModelsResult {
    pub models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 稳定机器码（仅需要界面按码翻译时携带）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

/// `POST /api/models/discover`：按 provider 预设调上游 `GET {base_url}/models` 取模型清单。
///
/// 凭据优先级：请求 `api_key` 非空 → 用请求值；否则 `provider_key_id` → 用已保存凭据
/// （归属校验，非本人 `404`）；两者皆无 → `200` + 空清单 + `error`，不发上游调用。
/// 获取失败不阻断保存：返回空清单 + `error` 说明。
pub async fn discover_models(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<DiscoverModels>,
) -> Result<Json<DiscoverModelsResult>> {
    let base = req.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Ok(Json(DiscoverModelsResult {
            models: vec![],
            error: Some("base_url 为空".to_string()),
            error_code: None,
        }));
    }

    let mut api_key = req.api_key.trim().to_string();
    if api_key.is_empty() {
        let key_id = req
            .provider_key_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        let Some(key_id) = key_id else {
            return Ok(Json(DiscoverModelsResult {
                models: vec![],
                error: Some(
                    "缺少凭据：请填写 API Key，或保存该 Provider 后使用已存凭据".to_string(),
                ),
                error_code: None,
            }));
        };
        let row = provider_keys::get(&state.pool, key_id)
            .await?
            .ok_or(ApiError::NotFound)?;
        // 归属校验：跨用户引用按记录不存在处理，绝不取用他人凭据。
        ensure_owned_route(&state, &user, &row.route_id).await?;
        // Service Account / AWS 凭据不支持列模型：绝不能把凭据内容当作 Bearer token 发出。
        if matches!(row.credential_kind.as_str(), "google_service_account" | "aws_sigv4") {
            return Ok(Json(DiscoverModelsResult {
                models: vec![],
                error: Some("该凭据类型不支持模型清单拉取".to_string()),
                error_code: Some("unsupported_credential_kind".to_string()),
            }));
        }
        api_key = state
            .cipher
            .decrypt(&row.credential_ciphertext, &aad_for(&row.id))?;
    }

    let auth_scheme = req
        .auth_scheme
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            presets::preset_of(&req.provider)
                .map(|preset| preset.auth_scheme.clone())
                // SA 需要换取 OAuth2 token，静态鉴权路径按 bearer 兜底。
                .filter(|scheme| scheme != "google_service_account")
        })
        .unwrap_or_else(|| "bearer".to_string());
    let api_formats = req
        .api_formats
        .unwrap_or_else(|| default_api_formats(presets::preset_of(&req.provider)));
    let outcome = discover_models_with(
        discover_family(&api_formats),
        base,
        &auth_scheme,
        &api_key,
        std::time::Duration::from_secs(15),
    )
    .await;
    Ok(Json(DiscoverModelsResult {
        models: outcome.models,
        error: outcome.error,
        error_code: outcome.error_code,
    }))
}

/// 由 `api_formats` 取一个 discover 分支：OpenAI > Anthropic；全缺省按 OpenAI。
fn discover_family(api_formats: &[String]) -> Family {
    let has = |format: &str| api_formats.iter().any(|item| item == format);
    if has("chat_completions") || has("responses") {
        Family::OpenAi
    } else if has("anthropic_messages") {
        Family::Anthropic
    } else {
        Family::OpenAi
    }
}

const UNSUPPORTED_MODEL_LIST: &str = "unsupported_model_list";
const UNSUPPORTED_MODEL_LIST_MESSAGE: &str = "上游不支持模型清单拉取，请手动添加模型";

/// 一次「模型清单获取」的结果；失败不返回 `Err`，由调用方决定如何呈现。
struct DiscoverOutcome {
    models: Vec<String>,
    error: Option<String>,
    error_code: Option<String>,
}

impl DiscoverOutcome {
    fn failure(error: String) -> Self {
        Self {
            models: vec![],
            error: Some(error),
            error_code: None,
        }
    }

    /// 证据确凿的「上游没有可解析的列模型端点」：`404` / `405` / `501` 或形状不符。
    fn unsupported() -> Self {
        Self {
            models: vec![],
            error: Some(UNSUPPORTED_MODEL_LIST_MESSAGE.to_string()),
            error_code: Some(UNSUPPORTED_MODEL_LIST.to_string()),
        }
    }
}

/// 手动「获取模型」与创建时自动拉取共用的上游调用（DRY）。
///
/// 地址按家族归一化后取 `{base}/models`（OpenAI 不补版本段，Anthropic 补 `/v1`）；
/// 响应只接受 OpenAI / Anthropic list（`data[].id`）。`timeout` 由调用方决定
///（手动 15 秒、创建时自动拉取 5 秒）。
async fn discover_models_with(
    family: Family,
    base_url: &str,
    auth_scheme: &str,
    api_key: &str,
    timeout: std::time::Duration,
) -> DiscoverOutcome {
    let base = normalize_base_url(base_url.trim(), family);
    if base.is_empty() {
        return DiscoverOutcome::failure("base_url 为空".to_string());
    }
    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(client) => client,
        Err(error) => {
            return DiscoverOutcome::failure(format!("构建 HTTP client 失败: {error}"));
        }
    };
    let url = format!("{base}/models");
    let request = client.get(&url);
    let request = match auth_scheme {
        "x-api-key" => request.header("x-api-key", api_key),
        _ => request.header(reqwest::header::AUTHORIZATION, format!("Bearer {api_key}")),
    };

    match request.send().await {
        Ok(response) => {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            if !status.is_success() {
                // 404 / 405 / 501 才是「端点不存在」的证据；401 / 403 等属鉴权问题。
                return if matches!(status.as_u16(), 404 | 405 | 501) {
                    DiscoverOutcome::unsupported()
                } else {
                    DiscoverOutcome::failure(format!("上游返回 {status}"))
                };
            }
            match parse_model_ids(&text) {
                Some(models) => DiscoverOutcome {
                    models,
                    error: None,
                    error_code: None,
                },
                None => DiscoverOutcome::unsupported(),
            }
        }
        Err(error) => DiscoverOutcome::failure(error.to_string()),
    }
}

/// 解析列模型形状：`data: [{id, …}]`（OpenAI / Anthropic，`display_name` 忽略）；
/// 形状不符返回 `None`。
fn parse_model_ids(body: &str) -> Option<Vec<String>> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let items = value.get("data").and_then(|data| data.as_array())?;
    Some(
        items
            .iter()
            .filter_map(|item| {
                item.get("id")
                    .and_then(|id| id.as_str())
                    .map(str::to_string)
                    .filter(|id| !id.is_empty())
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_accepts_any_http_prefix() {
        assert!(validate_base_url("https://api.openai.com/v1").is_ok());
        assert!(validate_base_url("https://example.com/compatible-mode/v1/").is_ok());
        assert!(validate_base_url("https://api.deepseek.com").is_ok());
        assert!(validate_base_url("https://api.deepinfra.com/v1/openai").is_ok());
        assert!(validate_base_url("http://localhost:11434/v1").is_ok());
        assert!(validate_base_url("api.deepseek.com").is_err());
        assert!(validate_base_url("ftp://api.deepseek.com").is_err());
        assert!(validate_base_url("https:///v1").is_err());
        assert!(validate_base_url("https://api.deepseek.com?x=1").is_err());
        assert!(validate_base_url("https://api.deepseek.com#x").is_err());
        assert!(validate_base_url("   ").is_err());
    }

    #[test]
    fn api_key_requires_non_empty_printable_ascii() {
        assert!(validate_api_key("sk-abc_123").is_ok());
        assert!(validate_api_key("sk-abc 123").is_ok());
        assert!(validate_api_key("   ").is_err());
        assert!(validate_api_key("sk-中文").is_err());
        assert!(validate_api_key("sk-\n").is_err());
    }

    #[test]
    fn models_are_trimmed_deduped_and_empties_dropped() {
        let raw = vec![
            ModelMapping {
                name: " gpt-5 ".to_string(),
                upstream: String::new(),
            },
            ModelMapping {
                name: "gpt-5".to_string(),
                upstream: "gpt-5".to_string(),
            },
            ModelMapping {
                name: String::new(),
                upstream: String::new(),
            },
            ModelMapping {
                name: "gpt-5-mini".to_string(),
                upstream: "gpt-5-mini-2025".to_string(),
            },
        ];
        let json = models_to_json(&raw);
        assert_eq!(
            json,
            r#"[{"name":"gpt-5","upstream":"gpt-5"},{"name":"gpt-5-mini","upstream":"gpt-5-mini-2025"}]"#
        );
        assert_eq!(
            models_from_json(&json),
            vec![
                ModelMapping {
                    name: "gpt-5".to_string(),
                    upstream: "gpt-5".to_string(),
                },
                ModelMapping {
                    name: "gpt-5-mini".to_string(),
                    upstream: "gpt-5-mini-2025".to_string(),
                },
            ]
        );
    }

    #[test]
    fn discover_family_prefers_openai_then_anthropic() {
        let formats = |list: &[&str]| list.iter().map(|value| value.to_string()).collect::<Vec<_>>();
        assert_eq!(discover_family(&formats(&[])), Family::OpenAi);
        assert_eq!(
            discover_family(&formats(&["provider_native"])),
            Family::OpenAi
        );
        assert_eq!(
            discover_family(&formats(&["anthropic_messages", "chat_completions"])),
            Family::OpenAi
        );
        assert_eq!(
            discover_family(&formats(&["anthropic_messages", "responses"])),
            Family::OpenAi
        );
        assert_eq!(
            discover_family(&formats(&["anthropic_messages"])),
            Family::Anthropic
        );
    }

    #[test]
    fn parse_model_ids_accepts_openai_anthropic_shape() {
        // OpenAI / Anthropic list：取 id，忽略 display_name。
        assert_eq!(
            parse_model_ids(r#"{"data":[{"id":"claude-x","display_name":"Claude X"},{"id":""}]}"#),
            Some(vec!["claude-x".to_string()])
        );
        // 形状命中但无 id：成功但空清单（由调用方决定是否算失败）。
        assert_eq!(parse_model_ids(r#"{"data":[]}"#), Some(vec![]));
        // Gemini `models[]` 形状不再接受。
        assert_eq!(
            parse_model_ids(r#"{"models":[{"name":"models/gemini-2.5-pro"}]}"#),
            None
        );
        // 形状不命中。
        assert_eq!(parse_model_ids(r#"{"error":{"message":"no"}}"#), None);
        assert_eq!(parse_model_ids("<html></html>"), None);
        assert_eq!(parse_model_ids(""), None);
    }
}

// ---------------- Usage ----------------

#[derive(Deserialize)]
pub struct UsageQuery {
    #[serde(default)]
    pub from: Option<i64>,
    #[serde(default)]
    pub to: Option<i64>,
    /// 1h 已移除；支持 24h/today/yesterday/7d/30d/this_month/last_month/custom。
    #[serde(default)]
    pub range: Option<String>,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub route_id: Option<String>,
    #[serde(default)]
    pub api_key_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    /// 仅全局统计使用。
    #[serde(default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
    /// 明细分页游标：`{at}:{request_id}`（R3-F8 键集分页；兼容旧 `at` 数字游标）。
    #[serde(default)]
    pub cursor: Option<String>,
}

struct UsageWindow {
    from: i64,
    to: i64,
    range: String,
    granularity: &'static str,
    group_seconds: i64,
    offset_minutes: i64,
}

fn local_offset(minutes: i64) -> UtcOffset {
    UtcOffset::from_whole_seconds((minutes * 60) as i32).unwrap_or(UtcOffset::UTC)
}

fn hour_start(ts: i64, offset_minutes: i64) -> i64 {
    let offset = offset_minutes * 60;
    ts - (ts + offset).rem_euclid(3600)
}

fn day_start(ts: i64, offset_minutes: i64) -> i64 {
    let offset = offset_minutes * 60;
    ts - (ts + offset).rem_euclid(86_400)
}

fn month_start(ts: i64, offset_minutes: i64) -> i64 {
    let offset = local_offset(offset_minutes);
    let Some(local) = OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .map(|value| value.to_offset(offset))
    else {
        return day_start(ts, offset_minutes);
    };
    let date = local.date();
    let first = Date::from_calendar_date(date.year(), date.month(), 1)
        .ok()
        .and_then(|date| date.with_hms(0, 0, 0).ok())
        .map(|value| value.assume_offset(offset))
        .map(|value| value.unix_timestamp())
        .unwrap_or_else(|| day_start(ts, offset_minutes));
    first
}

fn previous_month_start(ts: i64, offset_minutes: i64) -> i64 {
    let offset = local_offset(offset_minutes);
    let Some(local) = OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .map(|value| value.to_offset(offset))
    else {
        return month_start(ts - 30 * 86_400, offset_minutes);
    };
    let date = local.date();
    let month = u8::from(date.month());
    let (year, previous) = if month == 1 {
        (date.year() - 1, 12)
    } else {
        (date.year(), month - 1)
    };
    let Some(month) = Month::try_from(previous).ok() else {
        return month_start(ts, offset_minutes);
    };
    Date::from_calendar_date(year, month, 1)
        .ok()
        .and_then(|date| date.with_hms(0, 0, 0).ok())
        .map(|value| value.assume_offset(offset))
        .map(|value| value.unix_timestamp())
        .unwrap_or_else(|| month_start(ts, offset_minutes))
}

fn resolve_window(query: &UsageQuery, settings: &settings::UsageSettings) -> Result<UsageWindow> {
    // 聚合桶在采集时按全局设置对齐，查询必须使用同一偏移，不能由请求覆盖。
    let offset_minutes = settings.timezone_offset_minutes;
    let now = now_unix();
    let has_custom = query.from.is_some() || query.to.is_some();
    let range = query
        .range
        .as_deref()
        .unwrap_or(if has_custom { "custom" } else { "24h" });
    let today = day_start(now, offset_minutes);

    let (mut from, mut to) = match range {
        "24h" => (hour_start(now, offset_minutes) - 23 * 3600, now),
        "today" => (today, now),
        "yesterday" => (today - 86_400, today),
        "7d" => (today - 6 * 86_400, now),
        "30d" => (today - 29 * 86_400, now),
        "this_month" => (month_start(now, offset_minutes), now),
        "last_month" => {
            let start = month_start(now, offset_minutes);
            (previous_month_start(now, offset_minutes), start)
        }
        "custom" => {
            let from = query
                .from
                .ok_or_else(|| ApiError::InvalidRequest("custom 范围需要 from".to_string()))?;
            let to = query
                .to
                .ok_or_else(|| ApiError::InvalidRequest("custom 范围需要 to".to_string()))?;
            if from >= to {
                return Err(ApiError::InvalidRequest("from 必须早于 to".to_string()));
            }
            (from, to)
        }
        other => {
            return Err(ApiError::InvalidRequest(format!("不支持的 range: {other}")));
        }
    };

    if to <= from {
        return Err(ApiError::InvalidRequest("时间窗口为空".to_string()));
    }
    let span = to - from;
    let interval = query.interval.as_deref().unwrap_or("auto");
    let granularity = match interval {
        "hour" => "hour",
        "day" => "day",
        "auto" | "" => {
            if span > 90 * 86_400 {
                "day"
            } else {
                "hour"
            }
        }
        other => {
            return Err(ApiError::InvalidRequest(format!(
                "不支持的 interval: {other}"
            )));
        }
    };
    if granularity == "hour" && span > 365 * 86_400 {
        return Err(ApiError::InvalidRequest(
            "hour 粒度最长支持 365 天".to_string(),
        ));
    }
    if span > 730 * 86_400 {
        return Err(ApiError::InvalidRequest(
            "时间窗口最长支持 730 天".to_string(),
        ));
    }

    if range == "custom" {
        if granularity == "day" {
            from = day_start(from, offset_minutes);
            to = if to >= now {
                now
            } else {
                day_start(to, offset_minutes) + 86_400
            };
        } else {
            from = hour_start(from, offset_minutes);
            to = if to >= now {
                now
            } else {
                let start = hour_start(to, offset_minutes);
                if start == to { to } else { start + 3600 }
            };
        }
    }

    Ok(UsageWindow {
        from,
        to,
        range: range.to_string(),
        granularity,
        group_seconds: 0,
        offset_minutes,
    })
}

async fn scoped_filter<'a>(
    state: &AppState,
    user: &'a CurrentUser,
    query: &'a UsageQuery,
) -> Result<usage::UsageFilter<'a>> {
    if let Some(route_id) = query.route_id.as_deref() {
        ensure_owned_route(state, user, route_id).await?;
    }
    Ok(usage::UsageFilter {
        owner_id: Some(user.0.id.as_str()),
        route_id: query.route_id.as_deref(),
        api_key_id: query.api_key_id.as_deref(),
        model: query.model.as_deref(),
        provider_id: query.provider_id.as_deref(),
    })
}

fn global_filter(query: &UsageQuery) -> usage::UsageFilter<'_> {
    usage::UsageFilter {
        owner_id: query.owner_id.as_deref(),
        route_id: query.route_id.as_deref(),
        api_key_id: query.api_key_id.as_deref(),
        model: query.model.as_deref(),
        provider_id: query.provider_id.as_deref(),
    }
}

/// 用直方图近似百分位（返回该档上界）。
fn percentile(hist: &[i64; 11], pct: f64) -> i64 {
    let total: i64 = hist.iter().sum();
    if total == 0 {
        return 0;
    }
    let target = (total as f64 * pct).ceil() as i64;
    let mut acc = 0i64;
    for (index, count) in hist.iter().enumerate() {
        acc += count;
        if acc >= target {
            let upper = HIST_BUCKETS[index];
            return if upper == u64::MAX {
                (HIST_BUCKETS[index - 1] * 2) as i64
            } else {
                upper as i64
            };
        }
    }
    HIST_BUCKETS[9] as i64
}

#[derive(Serialize)]
pub struct UsageConfigResponse {
    pub timezone_offset_minutes: i64,
}

/// 任意已认证用户可读取统计口径（当前只有时区偏移）。
pub async fn usage_config(
    State(state): State<AppState>,
    _user: CurrentUser,
) -> Result<Json<UsageConfigResponse>> {
    let settings = settings::load_usage(&state.pool).await?;
    Ok(Json(UsageConfigResponse {
        timezone_offset_minutes: settings.timezone_offset_minutes,
    }))
}

#[derive(Serialize)]
pub struct UsageScope {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
}

#[derive(Serialize)]
pub struct UsageSummaryResponse {
    pub scope: UsageScope,
    pub from: i64,
    pub to: i64,
    pub range: String,
    pub tz_offset_minutes: i64,
    pub interval: &'static str,
    pub requests: i64,
    pub completed: i64,
    pub errors: i64,
    pub error_rate: f64,
    pub aborted: i64,
    pub stream_errors: i64,
    pub usage_missing: i64,
    pub failovers: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_write_tokens: i64,
    pub total_tokens: i64,
    pub latency_avg_ms: f64,
    pub ttft_avg_ms: f64,
    pub duration_avg_ms: f64,
    pub p50_ms: i64,
    pub p95_ms: i64,
    pub p99_ms: i64,
    pub bytes_in: i64,
    pub bytes_out: i64,
}

#[allow(clippy::too_many_arguments)]
fn summary_response(
    scope: UsageScope,
    window: &UsageWindow,
    range: String,
    totals: usage::UsageTotals,
) -> UsageSummaryResponse {
    let hist = totals.hist();
    let error_rate = if totals.requests > 0 {
        totals.errors as f64 / totals.requests as f64
    } else {
        0.0
    };
    let latency_avg_ms = if totals.latency_count > 0 {
        totals.latency_sum_ms as f64 / totals.latency_count as f64
    } else {
        0.0
    };
    let ttft_avg_ms = if totals.ttft_count > 0 {
        totals.ttft_sum_ms as f64 / totals.ttft_count as f64
    } else {
        0.0
    };
    let duration_avg_ms = if totals.duration_count > 0 {
        totals.duration_sum_ms as f64 / totals.duration_count as f64
    } else {
        0.0
    };
    UsageSummaryResponse {
        scope,
        from: window.from,
        to: window.to,
        range,
        tz_offset_minutes: window.offset_minutes,
        interval: window.granularity,
        requests: totals.requests,
        completed: totals.completed,
        errors: totals.errors,
        error_rate,
        aborted: totals.aborted,
        stream_errors: totals.stream_errors,
        usage_missing: totals.usage_missing,
        failovers: totals.failovers,
        input_tokens: totals.input_tokens,
        output_tokens: totals.output_tokens,
        cached_tokens: totals.cached_tokens,
        reasoning_tokens: totals.reasoning_tokens,
        cache_write_tokens: totals.cache_write_tokens,
        total_tokens: totals.input_tokens + totals.output_tokens,
        latency_avg_ms,
        ttft_avg_ms,
        duration_avg_ms,
        p50_ms: percentile(&hist, 0.50),
        p95_ms: percentile(&hist, 0.95),
        p99_ms: percentile(&hist, 0.99),
        bytes_in: totals.bytes_in,
        bytes_out: totals.bytes_out,
    }
}

pub async fn usage_summary(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageSummaryResponse>> {
    let settings = settings::load_usage(&state.pool).await?;
    let window = resolve_window(&query, &settings)?;
    let filter = scoped_filter(&state, &user, &query).await?;
    let totals = usage::totals(
        &state.pool,
        window.granularity,
        window.from,
        window.to,
        &filter,
    )
    .await?;
    Ok(Json(summary_response(
        UsageScope {
            kind: "self",
            owner_id: Some(user.0.id.clone()),
        },
        &window,
        window.range.clone(),
        totals,
    )))
}

pub async fn usage_timeseries(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Vec<usage::TimePoint>>> {
    let settings = settings::load_usage(&state.pool).await?;
    let window = resolve_window(&query, &settings)?;
    let filter = scoped_filter(&state, &user, &query).await?;
    Ok(Json(
        usage::timeseries(
            &state.pool,
            window.granularity,
            window.from,
            window.to,
            window.group_seconds,
            &filter,
        )
        .await?,
    ))
}

#[derive(Serialize)]
pub struct BreakdownDto {
    pub key: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_name: Option<String>,
    pub deleted: bool,
    pub archived: bool,
    pub requests: i64,
    pub completed: i64,
    pub errors: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub reasoning_tokens: i64,
    pub latency_avg_ms: f64,
    pub p95_ms: i64,
}

fn breakdown_dto(row: usage::BreakdownRow) -> BreakdownDto {
    let avg = if row.latency_count > 0 {
        row.latency_sum_ms as f64 / row.latency_count as f64
    } else {
        0.0
    };
    let p95_ms = percentile(&row.hist(), 0.95);
    BreakdownDto {
        key: row.key,
        label: row.label,
        route_id: row.route_id,
        route_name: row.route_name,
        owner_id: row.owner_id,
        owner_name: row.owner_name,
        deleted: row.deleted != 0,
        archived: row.archived != 0,
        requests: row.requests,
        completed: row.completed,
        errors: row.errors,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cached_tokens: row.cached_tokens,
        reasoning_tokens: row.reasoning_tokens,
        latency_avg_ms: avg,
        p95_ms,
    }
}

pub async fn usage_breakdown(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Vec<BreakdownDto>>> {
    let by = query.by.as_deref().unwrap_or("route");
    if by == "owner" {
        return Err(ApiError::InvalidRequest(
            "普通统计不支持按 owner 分组".to_string(),
        ));
    }
    if !matches!(by, "route" | "api_key" | "model" | "provider") {
        return Err(ApiError::InvalidRequest(format!("不支持的分组维度: {by}")));
    }
    let settings = settings::load_usage(&state.pool).await?;
    let window = resolve_window(&query, &settings)?;
    let filter = scoped_filter(&state, &user, &query).await?;
    let rows = usage::breakdown(
        &state.pool,
        window.granularity,
        window.from,
        window.to,
        by,
        &filter,
    )
    .await?;
    Ok(Json(rows.into_iter().map(breakdown_dto).collect()))
}

/// 把归属过滤（owner）解析成"该用户拥有的路由 id 集合"：明细在独立库，无法跨库 join。
async fn owner_route_ids(state: &AppState, owner_id: Option<&str>) -> Result<Option<Vec<String>>> {
    let Some(owner_id) = owner_id else {
        return Ok(None);
    };
    Ok(Some(
        routes::list_ids_by_owner(&state.pool, owner_id, true).await?,
    ))
}

/// 解析明细分页游标（R3-F8）：`{at}:{request_id}`；纯数字按旧 `at` 游标兼容
///（等价于「严格早于该秒」，避免同秒行重复）。
fn parse_detail_cursor(raw: &str) -> Option<(i64, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    match raw.split_once(':') {
        Some((at, id)) if !id.is_empty() => at.parse().ok().map(|at| (at, id.to_string())),
        Some(_) => None,
        None => raw.parse().ok().map(|at| (at, String::new())),
    }
}

/// 从明细库读取一页明细，并用主库补齐名称。
/// R2-A-6②：权限过滤已全部由 `owner_id` / `owner_route_ids` 完成，无需 `CurrentUser` 参数。
async fn load_detail_page(
    state: &AppState,
    query: &UsageQuery,
    window: &UsageWindow,
    owner_id: Option<&str>,
) -> Result<Vec<crate::db::detail::DetailView>> {
    let Some(store) = state.detail() else {
        return Ok(Vec::new());
    };
    let filter = crate::db::detail::DetailFilter {
        owner_id,
        owner_route_ids: owner_route_ids(state, owner_id).await?,
        route_id: query.route_id.as_deref(),
        api_key_id: query.api_key_id.as_deref(),
        model: query.model.as_deref(),
        provider_id: query.provider_id.as_deref(),
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let cursor = query.cursor.as_deref().and_then(parse_detail_cursor);
    let mut rows = crate::db::detail::list(
        store.pool(),
        window.from,
        window.to,
        limit,
        cursor.as_ref().map(|(at, id)| (*at, id.as_str())),
        &filter,
    )
    .await?;
    crate::db::detail::enrich_names(&state.pool, &mut rows).await?;
    Ok(rows.into_iter().map(crate::db::detail::to_view).collect())
}

pub async fn usage_requests(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Vec<crate::db::detail::DetailView>>> {
    let settings = settings::load_usage(&state.pool).await?;
    let window = resolve_window(&query, &settings)?;
    let filter = scoped_filter(&state, &user, &query).await?;
    Ok(Json(
        load_detail_page(&state, &query, &window, filter.owner_id).await?,
    ))
}

pub async fn admin_usage_summary(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageSummaryResponse>> {
    require_admin(&user)?;
    let settings = settings::load_usage(&state.pool).await?;
    let window = resolve_window(&query, &settings)?;
    let filter = global_filter(&query);
    let totals = usage::totals(
        &state.pool,
        window.granularity,
        window.from,
        window.to,
        &filter,
    )
    .await?;
    Ok(Json(summary_response(
        UsageScope {
            kind: "global",
            owner_id: None,
        },
        &window,
        window.range.clone(),
        totals,
    )))
}

pub async fn admin_usage_timeseries(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Vec<usage::TimePoint>>> {
    require_admin(&user)?;
    let settings = settings::load_usage(&state.pool).await?;
    let window = resolve_window(&query, &settings)?;
    let filter = global_filter(&query);
    Ok(Json(
        usage::timeseries(
            &state.pool,
            window.granularity,
            window.from,
            window.to,
            window.group_seconds,
            &filter,
        )
        .await?,
    ))
}

pub async fn admin_usage_breakdown(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Vec<BreakdownDto>>> {
    require_admin(&user)?;
    let by = query.by.as_deref().unwrap_or("route");
    if !matches!(by, "route" | "api_key" | "model" | "provider" | "owner") {
        return Err(ApiError::InvalidRequest(format!("不支持的分组维度: {by}")));
    }
    let settings = settings::load_usage(&state.pool).await?;
    let window = resolve_window(&query, &settings)?;
    let filter = global_filter(&query);
    let rows = usage::breakdown(
        &state.pool,
        window.granularity,
        window.from,
        window.to,
        by,
        &filter,
    )
    .await?;
    Ok(Json(rows.into_iter().map(breakdown_dto).collect()))
}

pub async fn admin_usage_requests(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(query): Query<UsageQuery>,
) -> Result<Json<Vec<crate::db::detail::DetailView>>> {
    require_admin(&user)?;
    let settings = settings::load_usage(&state.pool).await?;
    let window = resolve_window(&query, &settings)?;
    let filter = global_filter(&query);
    Ok(Json(
        load_detail_page(&state, &query, &window, filter.owner_id).await?,
    ))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ProviderHealthRow {
    pub provider_id: String,
    pub route_id: String,
    pub name: Option<String>,
    pub provider: String,
    pub state: String,
    pub reason: String,
    pub cooldown_until: Option<i64>,
    pub updated_at: i64,
    pub credential_kind: String,
    pub api_formats: String,
    pub models: String,
}

#[derive(Serialize)]
pub struct ProviderHealthDto {
    pub provider_id: String,
    pub route_id: String,
    pub name: Option<String>,
    pub provider: String,
    pub state: String,
    pub reason: String,
    pub cooldown_until: Option<i64>,
    pub updated_at: i64,
    /// 该凭据是否可被探针探测；false 表示"不可探测"，不得呈现为健康。
    pub probeable: bool,
}

/// 可探测判定：Service Account / 空模型清单 / 空 `api_formats` 均不可探测。
pub(crate) fn is_probeable(credential_kind: &str, api_formats_json: &str, models_json: &str) -> bool {
    if matches!(credential_kind, "google_service_account" | "aws_sigv4") {
        return false;
    }
    if models_from_json(models_json).is_empty() {
        return false;
    }
    api_formats_from_json(api_formats_json)
        .iter()
        .any(|format| {
            matches!(
                format.as_str(),
                "chat_completions" | "responses" | "anthropic_messages"
            )
        })
}

pub async fn list_provider_health(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<ProviderHealthDto>>> {
    let rows = sqlx::query_as::<_, ProviderHealthRow>(
        "SELECT h.provider_id, k.route_id, k.name, k.provider, h.state, h.reason,
                h.cooldown_until, h.updated_at, k.credential_kind, k.api_formats, k.models
         FROM provider_health h
         JOIN provider_keys k ON k.id = h.provider_id
         JOIN routes r ON r.id = k.route_id
         WHERE r.owner_id = ?1 AND r.archived_at IS NULL
         ORDER BY h.updated_at DESC",
    )
    .bind(&user.0.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| ProviderHealthDto {
                probeable: is_probeable(&row.credential_kind, &row.api_formats, &row.models),
                provider_id: row.provider_id,
                route_id: row.route_id,
                name: row.name,
                provider: row.provider,
                state: row.state,
                reason: row.reason,
                cooldown_until: row.cooldown_until,
                updated_at: row.updated_at,
            })
            .collect(),
    ))
}

/// 手动解除健康标记（仅 admin）：删行后即时下发，使凭据重新参与选号。
pub async fn clear_provider_health(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(provider_id): Path<String>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    if provider_keys::get(&state.pool, &provider_id)
        .await?
        .is_none()
    {
        return Err(ApiError::NotFound);
    }
    // admin 可在全站故障页解除任意 Provider 的健康标记；查询本身仍按 owner 隔离。
    provider_health::delete(&state.pool, &provider_id).await?;
    // 人工解除：同时关闭未解决故障记录（resolved_by = manual）。
    if let Ok(closed) =
        crate::db::incidents::close_open(&state.pool, &provider_id, now_unix(), "manual").await
        && !closed.is_empty()
    {
        crate::alerts::notify_closed(&state, &closed).await;
    }
    state.sync_core_best_effort().await;
    Ok(StatusCode::NO_CONTENT)
}
