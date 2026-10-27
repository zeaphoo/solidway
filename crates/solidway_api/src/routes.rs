use axum::Router;
use axum::middleware;
use axum::routing::{get, post};

use crate::auth::middleware as auth_middleware;
use crate::auth::routes as auth_routes;
use crate::error::ApiError;
use crate::handlers;
use crate::provider;
use crate::state::AppState;

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

/// 未知路径的兜底（无静态托管时）。
pub async fn not_found() -> ApiError {
    ApiError::NotFound
}

async fn api_not_found() -> ApiError {
    ApiError::NotFound
}

/// `/api` 子路由（未施加 state，由外层 `.with_state` 完成）。
pub fn api_router(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route(
            "/auth/me",
            get(auth_routes::me).patch(auth_routes::update_me),
        )
        .route("/auth/password", post(auth_routes::change_password))
        .route("/auth/logout", post(auth_routes::logout))
        // 版本号对任意已认证角色可见（系统页仅 admin）。
        .route("/version", get(handlers::version))
        .route("/access", get(handlers::get_access))
        .route("/system", get(handlers::system_info))
        .route("/app/info", get(handlers::app_info))
        .route("/app/restart", post(handlers::app_restart))
        .route(
            "/app/desktop",
            get(handlers::app_desktop_get).put(handlers::app_desktop_put),
        )
        .route("/network/addresses", get(handlers::network_addresses))
        .route("/provider-incidents", get(handlers::list_incidents))
        .route("/provider-incidents/stats", get(handlers::incident_stats))
        .route(
            "/admin/provider-incidents",
            get(handlers::admin_list_incidents),
        )
        .route(
            "/admin/provider-incidents/{id}/close",
            post(handlers::admin_close_incident),
        )
        .route("/tls/status", get(handlers::tls_status))
        .route("/tls/retry", post(handlers::tls_retry))
        .route(
            "/settings",
            get(handlers::get_settings).patch(handlers::patch_settings),
        )
        .route(
            "/users",
            get(handlers::list_users).post(handlers::create_user),
        )
        .route(
            "/users/{id}",
            axum::routing::patch(handlers::update_user).delete(handlers::delete_user),
        )
        .route("/users/{id}/password", post(handlers::reset_user_password))
        .route("/users/{id}/archive", post(handlers::archive_user))
        .route("/users/{id}/offboard", post(handlers::offboard_user))
        .route("/providers", get(provider::list_presets))
        .route("/models/discover", post(handlers::discover_models))
        .route("/usage/config", get(handlers::usage_config))
        .route("/usage/summary", get(handlers::usage_summary))
        .route("/usage/timeseries", get(handlers::usage_timeseries))
        .route("/usage/breakdown", get(handlers::usage_breakdown))
        .route("/usage/requests", get(handlers::usage_requests))
        .route("/admin/usage/summary", get(handlers::admin_usage_summary))
        .route(
            "/admin/usage/timeseries",
            get(handlers::admin_usage_timeseries),
        )
        .route(
            "/admin/usage/breakdown",
            get(handlers::admin_usage_breakdown),
        )
        .route("/admin/usage/requests", get(handlers::admin_usage_requests))
        .route("/provider-health", get(handlers::list_provider_health))
        .route(
            "/provider-health/{provider_id}",
            axum::routing::delete(handlers::clear_provider_health),
        )
        .route(
            "/routes",
            get(handlers::list_routes).post(handlers::create_route),
        )
        .route(
            "/routes/{id}",
            get(handlers::get_route)
                .patch(handlers::update_route)
                .delete(handlers::delete_route),
        )
        .route("/routes/{id}/archive", post(handlers::archive_route))
        .route("/routes/{id}/transfer", post(handlers::transfer_route))
        .route(
            "/routes/{id}/selection-strategy",
            get(handlers::get_selection_strategy).put(handlers::put_selection_strategy),
        )
        .route(
            "/routes/{id}/provider-keys",
            get(handlers::list_provider_keys).post(handlers::create_provider_key),
        )
        .route(
            "/provider-keys/{id}",
            get(handlers::get_provider_key)
                .patch(handlers::update_provider_key)
                .delete(handlers::delete_provider_key),
        )
        .route("/api-keys", get(handlers::list_api_keys))
        .route(
            "/api-keys/{id}",
            axum::routing::patch(handlers::update_api_key).delete(handlers::delete_api_key),
        )
        .route(
            "/routes/{id}/api-keys",
            get(handlers::list_route_api_keys).post(handlers::create_api_key),
        )
        .route_layer(middleware::from_fn_with_state(
            state,
            auth_middleware::require_auth,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/auth/login", post(auth_routes::login))
        // 引导接口必须公开：尚未设置口令时无会话可用。
        .route(
            "/auth/bootstrap",
            get(auth_routes::bootstrap_status).post(auth_routes::bootstrap),
        )
        // 桌面自动登录同样必须公开：它就是用来建立首个会话的（令牌门禁）。
        // 只暴露换取会话的 POST，不暴露任何「当前是否桌面模式」的探测接口。
        .route("/auth/desktop", post(auth_routes::desktop_login))
        .merge(protected)
        .fallback(api_not_found)
}
