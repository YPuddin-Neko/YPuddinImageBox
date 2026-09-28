//! HTTP 客户端与限速。
//!
//! - User-Agent 用「应用名/版本 (by 用户名)」。Danbooru 前面有 Cloudflare：浏览器 UA 会被
//!   拦成验证页（403），描述性 UA 可以正常访问，也符合 Danbooru 对 API 客户端的要求。
//! - 按用途分三条通道，各自限速、限并发：接口（搜索、计数）、预览图、原图下载。
//! - 收到 429 进入 60 秒退避，并把该通道速率永久减半一次；503 只退避 15 秒。
//! - 代理按设置生成客户端；改代理时换一个新客户端，正在进行的请求不受影响。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{PoisonError, RwLock};
use std::time::{Duration, Instant};

use reqwest::{RequestBuilder, Response, StatusCode};
use tokio::sync::{Mutex, Semaphore};

use crate::error::AppError;
use crate::i18n::tr;
use crate::settings::{parse_proxy_url, ProxyMode, ProxySettings};

pub const APP_UA: &str = concat!("YPuddinImageBox/", env!("CARGO_PKG_VERSION"));

/// 带账号时附上 `(by 用户名)`，站点按账号计速率，Cloudflare 收紧时也不容易被当成匿名流量。
pub fn user_agent(username: Option<&str>) -> String {
    match username.map(str::trim).filter(|u| !u.is_empty()) {
        Some(name) => format!("{APP_UA} (by {name})"),
        None => APP_UA.to_string(),
    }
}

/// 固定间隔发放令牌：每秒最多 `rate` 次，多个任务共享。
/// 只在计算下一个时间点时持锁，等待不占锁。
struct RateLimiter {
    state: Mutex<(Instant, Duration)>,
}

impl RateLimiter {
    fn per_second(rate: f64) -> Self {
        Self { state: Mutex::new((Instant::now(), Duration::from_secs_f64(1.0 / rate))) }
    }

    async fn acquire(&self) {
        let wait = {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            let slot = state.0.max(now);
            state.0 = slot + state.1;
            slot.saturating_duration_since(now)
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    async fn halve(&self) {
        let mut state = self.state.lock().await;
        state.1 *= 2;
    }
}

pub struct Lane {
    name: &'static str,
    limiter: RateLimiter,
    slots: Semaphore,
    backoff_until: Mutex<Instant>,
    halved: AtomicBool,
}

impl Lane {
    fn new(name: &'static str, rate_per_sec: f64, max_in_flight: usize) -> Self {
        Self {
            name,
            limiter: RateLimiter::per_second(rate_per_sec),
            slots: Semaphore::new(max_in_flight),
            backoff_until: Mutex::new(Instant::now()),
            halved: AtomicBool::new(false),
        }
    }

    async fn wait_turn(&self) {
        let until = *self.backoff_until.lock().await;
        let now = Instant::now();
        if until > now {
            tokio::time::sleep(until - now).await;
        }
        self.limiter.acquire().await;
    }

    async fn observe(&self, status: StatusCode) {
        let backoff = match status {
            StatusCode::TOO_MANY_REQUESTS => Duration::from_secs(60),
            StatusCode::SERVICE_UNAVAILABLE => Duration::from_secs(15),
            _ => return,
        };
        *self.backoff_until.lock().await = Instant::now() + backoff;
        if status == StatusCode::TOO_MANY_REQUESTS && !self.halved.swap(true, Ordering::SeqCst) {
            self.limiter.halve().await;
        }
        log::warn!("{} 通道收到 {}，暂停 {:?} 后再请求", self.name, status.as_u16(), backoff);
    }

    /// 按通道的并发、速率和退避状态发送请求。
    pub async fn send(&self, request: RequestBuilder) -> Result<Response, AppError> {
        let _permit = self.slots.acquire().await.expect("semaphore is never closed");
        self.wait_turn().await;
        let response = request.send().await?;
        self.observe(response.status()).await;
        Ok(response)
    }
}

pub struct Net {
    client: RwLock<reqwest::Client>,
    /// 搜索、计数等接口：2 次/秒，最多 4 个并发。
    pub api: Lane,
    /// 缩略图、预览图：10 次/秒，最多 6 个并发，保证瀑布流加载不卡。
    pub preview: Lane,
    /// 原图下载：5 次/秒，最多 4 个并发。
    pub file: Lane,
    /// Pixiv 的接口：1 次/秒，最多 2 个并发。Pixiv 对频繁请求限得很严，单独一条通道，
    /// 被限速时不拖慢其他站点。
    pub pixiv: Lane,
}

impl Net {
    pub fn new(proxy: &ProxySettings) -> Result<Self, AppError> {
        Ok(Self {
            client: RwLock::new(build_client(proxy)?),
            api: Lane::new("接口", 2.0, 4),
            preview: Lane::new("预览", 10.0, 6),
            file: Lane::new("下载", 5.0, 4),
            pixiv: Lane::new("Pixiv", 1.0, 2),
        })
    }

    /// 当前的 HTTP 客户端（内部是共享的连接池，复制很便宜）。
    pub fn client(&self) -> reqwest::Client {
        self.client.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// 按新的代理设置换客户端，之后发出的请求生效。
    pub fn apply_proxy(&self, proxy: &ProxySettings) -> Result<(), AppError> {
        let client = build_client(proxy)?;
        *self.client.write().unwrap_or_else(PoisonError::into_inner) = client;
        Ok(())
    }
}

/// 「跟随系统」时 reqwest 读取系统代理设置和 HTTP(S)_PROXY 环境变量；手动设置了代理就只用它。
pub fn build_client(proxy: &ProxySettings) -> Result<reqwest::Client, AppError> {
    let builder = reqwest::Client::builder()
        .user_agent(user_agent(None))
        .gzip(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60));
    let builder = match proxy.mode {
        ProxyMode::System => builder,
        ProxyMode::None => builder.no_proxy(),
        ProxyMode::Manual => {
            let url = parse_proxy_url(&proxy.url)?;
            let proxy = reqwest::Proxy::all(url.as_str())
                .map_err(|e| {
                    let detail = network_detail(&e);
                    AppError::InvalidInput(tr!("代理地址无效：{detail}", "Invalid proxy address: {detail}"))
                })?;
            builder.proxy(proxy)
        }
    };
    Ok(builder.build()?)
}

/// 用给定的代理设置访问一次 Danbooru，返回耗时。只发一个很小的请求，不经过限速通道。
pub async fn test_connection(proxy: &ProxySettings) -> Result<Duration, AppError> {
    let client = build_client(proxy)?;
    let start = Instant::now();
    let response =
        client.get("https://danbooru.donmai.us/robots.txt").timeout(Duration::from_secs(15)).send().await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Http { site: "Danbooru", status: status.as_u16() });
    }
    Ok(start.elapsed())
}

/// 网络错误的说明：超时、连不上直接说清楚，其余给出最底层的原因（例如 dns error）。
pub fn network_detail(err: &reqwest::Error) -> String {
    if err.is_timeout() {
        return tr!("连接超时", "The connection timed out");
    }
    let mut root: &dyn std::error::Error = err;
    while let Some(next) = root.source() {
        root = next;
    }
    if err.is_connect() {
        tr!(
            "连不上服务器（{root}），请检查网络或代理设置",
            "Can't reach the server ({root}). Check your network or proxy settings"
        )
    } else {
        root.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_appends_username() {
        assert_eq!(user_agent(None), APP_UA);
        assert_eq!(user_agent(Some("  ")), APP_UA);
        assert_eq!(user_agent(Some("sora")), format!("{APP_UA} (by sora)"));
    }

    #[tokio::test]
    async fn limiter_spaces_out_requests() {
        let limiter = RateLimiter::per_second(20.0);
        let start = Instant::now();
        for _ in 0..5 {
            limiter.acquire().await;
        }
        // 第 1 次立即放行，其余每次间隔 50ms。
        assert!(start.elapsed() >= Duration::from_millis(190));
    }
}
