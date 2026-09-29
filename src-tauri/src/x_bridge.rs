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
static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

pub fn set_capture(capture: Capture) {
    *CAPTURE.lock().unwrap_or_else(PoisonError::into_inner) = Some(capture);
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
    let capture = CAPTURE.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let Some(capture) = capture.filter(|capture| capture.accepts(&payload.page)) else { return Ok(()) };
    let posts = sources::x::parse_posts(&payload.body, Some(&capture))?;
    log::debug!("X 采集：{} 收到 {} 张图（{}）", payload.page, posts.len(), payload.path.rsplit('/').next().unwrap_or_default());
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
pub const INIT_SCRIPT: &str = r#"
(() => {
  if (!/^(https?:\/\/)(x\.com|[^/]+\.x\.com|twitter\.com|[^/]+\.twitter\.com)\//.test(location.href)) return;
  const graphql = /\/graphql\/[^/]+\/[^/]+$/;
  const urlOf = (input) => {
    try {
      return new URL(input instanceof Request ? input.url : String(input), location.href);
    } catch (_) {
      return null;
    }
  };
  const sent = new Set();
  const send = (url, body) => {
    try {
      if (!url || !graphql.test(url.pathname) || !body || body.indexOf('media_url_https') < 0) return;
      const key = url.pathname + ':' + body.length + ':' + body.slice(0, 32);
      if (sent.has(key)) return;
      sent.add(key);
      if (sent.size > 300) sent.delete(sent.values().next().value);
      const invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
      if (typeof invoke === 'function') {
        invoke('plugin:x|bridge_response', { payload: { path: url.pathname, page: location.pathname, body } }).catch(() => {});
      }
    } catch (_) {}
  };

  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(method, url) {
    const request = this;
    const parsed = urlOf(url);
    if (parsed && graphql.test(parsed.pathname)) {
      request.addEventListener('load', () => {
        if (request.status === 200 && typeof request.responseText === 'string') send(parsed, request.responseText);
      });
    }
    return originalOpen.apply(this, arguments);
  };

  const originalFetch = window.fetch;
  window.fetch = async function(input) {
    const response = await originalFetch.apply(this, arguments);
    const parsed = urlOf(input);
    if (parsed && graphql.test(parsed.pathname) && response.ok) {
      response.clone().text().then(body => send(parsed, body)).catch(() => {});
    }
    return response;
  };

  let lastHeight = 0;
  let stable = 0;
  const scrollTimer = setInterval(() => {
    if (document.visibilityState === 'hidden') return;
    const height = document.documentElement.scrollHeight;
    window.scrollTo(0, height);
    stable = height === lastHeight ? stable + 1 : 0;
    lastHeight = height;
    if (stable >= 12) clearInterval(scrollTimer);
  }, 1500);
})();
"#;
