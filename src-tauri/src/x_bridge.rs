//! X 页面桥接窗口。
//!
//! X 的时间线接口属于网页内部实现，桥接窗口只把当前页面已经收到的时间线响应
//! 交给解析器。插件命令单独注册，只有 X 远程窗口能访问它。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime, WebviewWindow};

use crate::error::AppError;
use crate::i18n::tr;
use crate::sources::{self, Post};

pub const WINDOW: &str = "x-capture";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeResponse {
    pub path: String,
    pub body: String,
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
    let Some(kind) = sources::x::timeline_kind(&payload.path) else { return Ok(()) };
    let posts = sources::x::parse_posts(&payload.body)?;
    if !posts.is_empty() {
        app.emit("x-posts", PostsPayload { posts, kind })
            .map_err(|err| AppError::Internal(err.to_string()))?;
    }
    Ok(())
}

pub fn init<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("x")
        .invoke_handler(tauri::generate_handler![bridge_response])
        .build()
}

/// 在 document_start 阶段安装拦截器。它只捕获 X 自己的时间线 GraphQL，
/// 不读取 Cookie，也不改写响应内容。
pub const INIT_SCRIPT: &str = r#"
(() => {
  if (!/^(https?:\/\/)(x\.com|[^/]+\.x\.com|twitter\.com|[^/]+\.twitter\.com)\//.test(location.href)) return;
  const pathPattern = /(?:^|\/)graphql\/[^/]+\/(?:UserMedia|UserTweets|TweetDetail|TweetResultByRestId|Likes|Bookmarks)(?:$|\/)/;
  const sent = new Set();
  const send = (url, body) => {
    try {
      const parsed = new URL(url, location.href);
      if (!pathPattern.test(parsed.pathname) || !body || body.length < 20) return;
      const key = parsed.pathname + ':' + body.length + ':' + body.slice(0, 32);
      if (sent.has(key)) return;
      sent.add(key);
      if (sent.size > 300) sent.delete(sent.values().next().value);
      const invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
      if (typeof invoke === 'function') {
        invoke('plugin:x|bridge_response', { payload: { path: parsed.pathname, body } }).catch(() => {});
      }
    } catch (_) {}
  };

  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(method, url) {
    const request = this;
    try {
      const parsed = new URL(url, location.href);
      if (pathPattern.test(parsed.pathname)) {
        request.addEventListener('load', () => {
          if (request.status === 200 && typeof request.responseText === 'string') send(parsed.href, request.responseText);
        });
      }
    } catch (_) {}
    return originalOpen.apply(this, arguments);
  };

  const originalFetch = window.fetch;
  window.fetch = async function() {
    const response = await originalFetch.apply(this, arguments);
    try {
      const request = arguments[0];
      const url = typeof request === 'string' ? request : request && request.url;
      const parsed = new URL(url, location.href);
      if (pathPattern.test(parsed.pathname)) response.clone().text().then(body => send(parsed.href, body)).catch(() => {});
    } catch (_) {}
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
