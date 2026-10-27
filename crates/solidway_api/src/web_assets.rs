//! 前端静态资源：`--web-assets` > 编译期默认（debug 读盘 / release 内置）。
//!
//! `rust-embed` 的动态分支（debug）使用**编译期解析出的绝对路径**，因此与进程
//! cwd 无关；release 则把资产编进二进制。

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

#[cfg(not(debug_assertions))]
#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
// 干净检出（未跑 pnpm build）时目录不存在：不阻断编译，运行时按 404 处理。
#[allow_missing = true]
struct Embedded;

/// 资产来源。
#[derive(Clone)]
enum Source {
    Disk(PathBuf),
    #[cfg(not(debug_assertions))]
    Embedded,
}

/// 静态资源服务句柄。
#[derive(Clone)]
pub struct WebAssets {
    source: Source,
    files: usize,
}

/// `(source, path, files)`：供 `/api/system` 自诊断。
#[derive(Debug, Clone)]
pub struct Descriptor {
    pub source: &'static str,
    pub path: Option<String>,
    pub files: usize,
}

impl WebAssets {
    /// 解析资产来源。`override_dir` 来自 `--web-assets`。
    pub fn resolve(override_dir: Option<PathBuf>) -> Result<Self, String> {
        if let Some(dir) = override_dir {
            if !dir.join("index.html").is_file() {
                return Err(format!(
                    "--web-assets 指定的目录缺少 index.html：{}",
                    dir.display()
                ));
            }
            let dir = crate::absolute_path(&dir);
            let files = count_disk(&dir);
            return Ok(Self {
                source: Source::Disk(dir),
                files,
            });
        }

        #[cfg(not(debug_assertions))]
        {
            let files = Embedded::iter().count();
            Ok(Self {
                source: Source::Embedded,
                files,
            })
        }

        #[cfg(debug_assertions)]
        {
            let dir = crate::absolute_path(Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../web/dist"
            )));
            if !dir.join("index.html").is_file() {
                tracing::warn!(path = %dir.display(), "frontend build assets not found; serving API only");
            }
            let files = count_disk(&dir);
            Ok(Self {
                source: Source::Disk(dir),
                files,
            })
        }
    }

    pub fn describe(&self) -> Descriptor {
        match &self.source {
            Source::Disk(dir) => Descriptor {
                source: "disk",
                path: Some(dir.display().to_string()),
                files: self.files,
            },
            #[cfg(not(debug_assertions))]
            Source::Embedded => Descriptor {
                source: "embedded",
                path: None,
                files: self.files,
            },
        }
    }

    async fn read(&self, path: &str) -> Option<Vec<u8>> {
        match &self.source {
            // 磁盘模式不能阻塞 Tokio worker；静态资源很小，异步读足够。
            Source::Disk(dir) => tokio::fs::read(dir.join(path)).await.ok(),
            #[cfg(not(debug_assertions))]
            Source::Embedded => Embedded::get(path).map(|file| file.data.into_owned()),
        }
    }
}

/// 静态资源 + SPA 回退路由（`/api` 由外层先行挂载）。
pub fn router(assets: WebAssets) -> Router {
    Router::new().fallback(get(serve)).with_state(assets)
}

async fn serve(State(assets): State<WebAssets>, uri: Uri) -> Response {
    let requested = uri.path().trim_start_matches('/');

    // 路径穿越防护（磁盘分支尤其需要）。
    if requested.split('/').any(|segment| segment == "..") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    let requested = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };

    let hit = assets.read(requested).await;
    let (body, served_path, from_index) = match hit {
        Some(bytes) => (bytes, requested.to_string(), false),
        None => match assets.read("index.html").await {
            Some(index) => (index, "index.html".to_string(), true),
            None => return (StatusCode::NOT_FOUND, "console not built").into_response(),
        },
    };
    let _ = from_index;

    let cache = cache_control(&served_path);
    (
        [
            (header::CONTENT_TYPE, mime_of(&served_path)),
            (header::CACHE_CONTROL, cache),
        ],
        Body::from(body),
    )
        .into_response()
}

/// `index.html` 不缓存（否则发版后可能引用已删除的 hash 资产）；
/// vite 产出的 `assets/*` 带内容哈希，可长缓存。
fn cache_control(path: &str) -> &'static str {
    if path == "index.html" {
        "no-cache"
    } else if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

fn mime_of(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") | Some("map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 统计目录下的文件数（仅用于自诊断展示）。
fn count_disk(root: &Path) -> usize {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                total += 1;
            }
        }
    }
    total
}
