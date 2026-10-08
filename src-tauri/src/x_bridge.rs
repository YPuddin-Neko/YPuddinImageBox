//! X 页面桥接窗口。
//!
//! X 的时间线接口属于网页内部实现，桥接窗口只把当前页面已经收到的、带图片的 GraphQL 响应
//! 交给解析器。插件命令单独注册，只有 X 远程窗口能访问它。

use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime, WebviewWindow};
use url::Url;

use crate::error::AppError;
use crate::i18n::tr;
use crate::sources::x::Capture;
use crate::sources::{self, Post, Source};

pub const WINDOW: &str = "x-capture";

/// 采集窗口这次打开是为了采什么；换目标时（例如收藏页打开喜欢）窗口不关，只换这里。
#[derive(Clone)]
struct CaptureSession {
    capture: Capture,
    page: String,
}

static CAPTURE: Mutex<Option<CaptureSession>> = Mutex::new(None);

pub fn set_capture(capture: Capture, page: &str) {
    *CAPTURE.lock().unwrap_or_else(PoisonError::into_inner) = Some(CaptureSession { capture, page: page.to_string() });
}

pub fn configure_window<R: Runtime>(window: &WebviewWindow<R>) {
    let session = CAPTURE.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let Some(session) = session else { return };
    let config = serde_json::json!({
        "page": session.page,
        "start": tr!("开始自动下拉", "Start auto-scroll"),
        "pause": tr!("暂停自动下拉", "Pause auto-scroll"),
    });
    let script = format!("window.__IMAGEBOX_X_CAPTURE__?.configure({config});");
    if let Err(err) = window.eval(&script) {
        log::debug!("X 采集窗口控制未就绪：{err}");
    }
}

/// X 登录页要用到的第三方页面：Google、Apple 登录（按钮本身就是 accounts.google.com 的框架），
/// 以及人机验证。macOS 上页面里的框架也要经过导航检查，不放行的话这些按钮和验证都显示不出来。
const LOGIN_HOSTS: [&str; 5] = ["accounts.google.com", "accounts.youtube.com", "appleid.apple.com", "arkoselabs.com", "challenges.cloudflare.com"];

fn login_host(host: &str) -> bool {
    LOGIN_HOSTS.iter().any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

/// 采集窗口能打开的地址：X 自己的页面、登录要用的页面，以及框架常用的空白页。点开帖子里的外链不在窗口里打开。
pub fn allows_navigation(url: &Url) -> bool {
    match url.scheme() {
        "about" | "blob" | "data" => true,
        "https" => url.host_str().is_some_and(|host| Source::for_host(host) == Some(Source::X) || login_host(host)),
        _ => false,
    }
}

/// 用 Google、Apple 登录会弹出登录窗口；其他弹窗不开。
pub fn allows_popup(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str().is_some_and(login_host)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeResponse {
    pub path: String,
    pub body: String,
    /// 收到响应时采集窗口停在哪个页面（`location.pathname`）。
    #[serde(default)]
    pub page: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostsPayload {
    pub posts: Vec<Post>,
    /// `media`、`likes` 或 `bookmarks`，见 [`sources::x::timeline_kind`]。
    pub kind: &'static str,
}

/// 只允许采集窗口把 X 的时间线响应送进应用，不能从页面触发其他应用命令。
#[tauri::command]
pub async fn bridge_response<R: Runtime>(
    app: AppHandle<R>,
    webview: WebviewWindow<R>,
    payload: BridgeResponse,
) -> Result<(), AppError> {
    if webview.label() != WINDOW {
        return Err(AppError::InvalidInput(tr!(
            "只能从 X 采集窗口提交响应",
            "Only the X capture window can submit responses"
        )));
    }
    let query = payload.path.rsplit('/').next().unwrap_or_default();
    let capture = CAPTURE.lock().unwrap_or_else(PoisonError::into_inner).as_ref().map(|session| session.capture.clone());
    let Some(capture) = capture.filter(|capture| capture.accepts(&payload.page)) else {
        log::debug!("X 采集：{} 不是这次要采的页面，跳过 {query}", payload.page);
        return Ok(());
    };
    // X 的网页常改，收到了什么、解析出几张都记进日志，采不到图时看日志就知道是哪一步没对上。
    let posts = sources::x::parse_posts(&payload.body, Some(&capture)).inspect_err(|err| {
        log::warn!("X 采集：{} 的 {query} 解析失败：{err}", payload.page);
    })?;
    log::info!("X 采集：{} 的 {query} 里有 {} 张图", payload.page, posts.len());
    if !posts.is_empty() {
        app.emit("x-posts", PostsPayload { posts, kind: capture.kind() })
            .map_err(|err| AppError::Internal(err.to_string()))?;
    }
    Ok(())
}

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("x")
        .invoke_handler(tauri::generate_handler![bridge_response])
        .build()
}

/// 在 document_start 阶段安装拦截器。它只捕获 X 自己的 GraphQL 响应里带图片的那些，
/// 不读取 Cookie，也不改写响应内容。
///
/// 2026 年 9 月 X 换了新网页（Relay）：请求地址是 `api.x.com/graphql/{id}/{查询名}`，查询名也换了一套，
/// 而且 fetch 传进来的是 URL 对象不是字符串。所以不按查询名挑，只看是不是 GraphQL、响应里有没有图片地址，
/// 归到哪里由 Rust 按采集窗口停在哪个页面决定。
pub const INIT_SCRIPT: &str = include_str!("x_capture.js");
