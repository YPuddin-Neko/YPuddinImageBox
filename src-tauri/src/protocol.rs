//! `ibx://` 图片协议：界面上的图片都经这里加载。
//!
//! - 远程图：界面不直接请求站点图床，这样代理、UA、Referer、限速和域名白名单只在一处生效；
//!   加载过的图缓存在「缓存」位置下的 remote 目录（14 天）。
//! - 查看器里的原图：`full/{完整地址}`。照样先查缓存，但不写进去（原图常有几十 MB，缓存会很快膨胀），
//!   大小上限和超时也放宽一些。
//! - 图库里的图：`local/thumb/{来源}/{帖子id}` 和 `local/file/{来源}/{帖子id}`，
//!   只能读到图库登记过的文件；缩略图不存在时现场生成。发现页里「已下载」的图在查看器里用
//!   `local/file/{来源}/{帖子id}/{md5}`：同一张图可能是从别的站点下载的，按帖子找不到文件时再按 md5 找。
//!
//! 地址由前端 `convertFileSrc(地址, "ibx")` 生成，路径部分是百分号编码后的完整地址。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};
use percent_encoding::percent_decode_str;
use reqwest::header::REFERER;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime};
use url::Url;

use crate::sources::{self, fanbox, Source};
use crate::storage::StorageKind;
use crate::{thumbs, AppState};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_FULL_BYTES: usize = 64 * 1024 * 1024;
/// 原图可能很大，不按客户端默认的 60 秒超时算。
const FULL_TIMEOUT: Duration = Duration::from_secs(180);
const CACHE_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);

type Failure = (StatusCode, String);

pub async fn serve<R: Runtime>(app: &AppHandle<R>, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let result = load(app, request.uri().path()).await;
    let cache_control = if is_fanbox_path(request.uri().path()) { "no-store" } else { "private, max-age=604800" };
    let builder = Response::builder();
    match result {
        Ok((bytes, mime)) => builder
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, cache_control)
            .body(bytes),
        Err((status, message)) => {
            if status.is_server_error() {
                log::warn!("图片加载失败 {}：{message}", status.as_u16());
            } else {
                log::debug!("图片加载失败 {}：{message}", status.as_u16());
            }
            builder
                .status(status)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(message.into_bytes())
        }
    }
    .expect("static response parts are valid")
}

async fn load<R: Runtime>(app: &AppHandle<R>, path: &str) -> Result<(Vec<u8>, &'static str), Failure> {
    let raw = percent_decode_str(path.trim_start_matches('/'))
        .decode_utf8()
        .map_err(|_| (StatusCode::BAD_REQUEST, "图片地址编码无效".to_string()))?;
    if let Some(route) = raw.strip_prefix("local/") {
        return load_local(app, route).await;
    }
    let (raw, full) = raw.strip_prefix("full/").map_or((&*raw, false), |rest| (rest, true));
    let limit = if full { MAX_FULL_BYTES } else { MAX_BYTES };
    let too_large = || (StatusCode::PAYLOAD_TOO_LARGE, format!("图片超过 {} MB", limit >> 20));
    let url = Url::parse(raw).map_err(|_| (StatusCode::BAD_REQUEST, "图片地址无效".to_string()))?;
    let source =
        sources::source_for_url(&url).ok_or((StatusCode::FORBIDDEN, "只能加载已接入站点的图片".to_string()))?;

    let state = app.state::<AppState>();
    let accounts = state.accounts.get();
    let session = (source == Source::Fanbox).then(|| accounts.fanbox.as_ref().map(|c| c.session.as_str())).flatten();
    // FANBOX 预览按会话分开缓存，切换账号后重新检查图片访问权限。
    let cache = cache_path(app, &url, session);
    if let Some(bytes) = read_fresh(cache.as_deref()).await {
        if let Some(mime) = sniff(&bytes) {
            return Ok((bytes, mime));
        }
    }

    let mut request = if source == Source::Fanbox {
        fanbox::media_request(&state.net, url.clone(), accounts.fanbox.as_ref())
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    } else {
        state.net.client().get(url.clone()).header(REFERER, source.referer())
    };
    if full {
        request = request.timeout(FULL_TIMEOUT);
    }
    let mut response = state.net.preview.send(request).await.map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    if !response.status().is_success() {
        return Err((StatusCode::BAD_GATEWAY, format!("{} 返回 HTTP {}", source.site_name(), response.status().as_u16())));
    }
    if response.content_length().is_some_and(|len| len as usize > limit) {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > limit {
            return Err(too_large());
        }
    }
    // 以文件内容判断格式，不信任上游的 Content-Type。
    let mime = sniff(&bytes).ok_or((StatusCode::BAD_GATEWAY, "不是支持的图片格式".to_string()))?;
    if let Some(path) = cache.filter(|_| !full) {
        write_atomic(&path, &bytes).await;
    }
    Ok((bytes, mime))
}

#[derive(Debug, PartialEq, Eq)]
enum LocalRoute {
    Thumb,
    File,
}

#[derive(Debug, PartialEq, Eq)]
struct LocalRequest {
    kind: LocalRoute,
    source: Source,
    id: u64,
    /// 只有原图能带：按帖子找不到文件时按它找同一张图。
    md5: Option<String>,
}

fn parse_local(route: &str) -> Option<LocalRequest> {
    let mut parts = route.split('/');
    let kind = match parts.next()? {
        "thumb" => LocalRoute::Thumb,
        "file" => LocalRoute::File,
        _ => return None,
    };
    let source = Source::parse(parts.next()?)?;
    let id = parts.next()?.parse().ok()?;
    let md5 = match parts.next() {
        None => None,
        Some(md5) if kind == LocalRoute::File && md5.len() == 32 && md5.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some(md5.to_ascii_lowercase())
        }
        Some(_) => return None,
    };
    parts.next().is_none().then_some(LocalRequest { kind, source, id, md5 })
}

async fn load_local<R: Runtime>(app: &AppHandle<R>, route: &str) -> Result<(Vec<u8>, &'static str), Failure> {
    let LocalRequest { kind, source, id, md5 } =
        parse_local(route).ok_or((StatusCode::BAD_REQUEST, "图片地址无效".to_string()))?;
    let state = app.state::<AppState>();
    let database = |e: sqlx::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    let own = state.library.local_path(source, id).await.map_err(database)?;
    let not_found = || (StatusCode::NOT_FOUND, "图库里没有这张图".to_string());
    let missing = || (StatusCode::NOT_FOUND, "图片文件不见了，可能已被移动或删除".to_string());

    if kind == LocalRoute::File {
        // 先读这个帖子自己的文件，没有（不在图库里，或文件不见了）再读 md5 相同的。
        let mut candidates: Vec<PathBuf> = own.into_iter().collect();
        if let Some(md5) = md5 {
            candidates.extend(state.library.paths_with_md5(&md5, source, id).await.map_err(database)?);
        }
        if candidates.is_empty() {
            return Err(not_found());
        }
        for path in candidates {
            if let Ok(bytes) = tokio::fs::read(&path).await {
                let mime = sniff(&bytes).ok_or((StatusCode::UNSUPPORTED_MEDIA_TYPE, "不是支持的图片格式".to_string()))?;
                return Ok((bytes, mime));
            }
        }
        return Err(missing());
    }

    let file = own.ok_or_else(not_found)?;
    let thumb = thumbs::path(&state.storage().path(StorageKind::Cache), source, id);
    if let Ok(bytes) = tokio::fs::read(&thumb).await {
        if let Some(mime) = sniff(&bytes) {
            return Ok((bytes, mime));
        }
    }
    if !tokio::fs::try_exists(&file).await.unwrap_or(false) {
        return Err(missing());
    }
    // 解码不了的格式（例如 AVIF）直接用原图当缩略图，交给 WebView 显示。
    if let Ok(bytes) = thumbs::generate(file.clone(), thumb).await {
        if let Some(mime) = sniff(&bytes) {
            return Ok((bytes, mime));
        }
    }
    let bytes = tokio::fs::read(&file).await.map_err(|_| missing())?;
    let mime = sniff(&bytes).ok_or((StatusCode::UNSUPPORTED_MEDIA_TYPE, "不是支持的图片格式".to_string()))?;
    Ok((bytes, mime))
}

fn is_fanbox_path(path: &str) -> bool {
    let Ok(raw) = percent_decode_str(path.trim_start_matches('/')).decode_utf8() else { return false };
    let raw = raw.strip_prefix("full/").unwrap_or(&raw);
    Url::parse(raw).ok().and_then(|url| sources::source_for_url(&url)) == Some(Source::Fanbox)
}

fn cache_key(url: &Url, session: Option<&str>) -> String {
    let mut digest = Md5::new();
    digest.update(url.as_str().as_bytes());
    if let Some(session) = session {
        digest.update([0]);
        digest.update(session.as_bytes());
    }
    digest.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn cache_path<R: Runtime>(app: &AppHandle<R>, url: &Url, session: Option<&str>) -> Option<PathBuf> {
    let hex = cache_key(url, session);
    let root = app.state::<AppState>().storage().path(StorageKind::Cache);
    Some(root.join("remote").join(&hex[..2]).join(hex))
}

async fn read_fresh(path: Option<&Path>) -> Option<Vec<u8>> {
    let path = path?;
    let modified = tokio::fs::metadata(path).await.ok()?.modified().ok()?;
    if modified.elapsed().map_or(true, |age| age > CACHE_TTL) {
        return None;
    }
    tokio::fs::read(path).await.ok()
}

/// 写缓存失败不影响本次显示，所以错误直接忽略。
async fn write_atomic(path: &Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else { return };
    if tokio::fs::create_dir_all(parent).await.is_err() {
        return;
    }
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let temp = path.with_extension(format!("{}.{nanos}.part", std::process::id()));
    if tokio::fs::write(&temp, bytes).await.is_ok() && tokio::fs::rename(&temp, path).await.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    }
}

/// 按文件头判断图片格式。
pub(crate) fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        [_, _, _, _, b'f', b't', b'y', b'p', b'a', b'v', b'i', b'f' | b's', ..] => Some("image/avif"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fanbox_remote_cache_is_separated_by_session() {
        let url = Url::parse("https://downloads.fanbox.cc/images/post/1/a.jpeg").unwrap();
        assert_ne!(cache_key(&url, Some("first")), cache_key(&url, Some("second")));
        assert_ne!(cache_key(&url, Some("first")), cache_key(&url, None));
        assert!(is_fanbox_path("/full%2Fhttps%3A%2F%2Fdownloads.fanbox.cc%2Fimages%2Fa.jpeg"));
        assert!(!is_fanbox_path("/local/file/fanbox/1000"));
        assert!(!is_fanbox_path("/https%3A%2F%2Fdownloads.fanbox.cc.evil.test%2Fa.jpeg"));
    }

    #[test]
    fn parses_local_routes_strictly() {
        let request = |kind, source, id, md5: Option<&str>| Some(LocalRequest { kind, source, id, md5: md5.map(String::from) });
        assert_eq!(parse_local("thumb/danbooru/42"), request(LocalRoute::Thumb, Source::Danbooru, 42, None));
        assert_eq!(parse_local("file/gelbooru/7"), request(LocalRoute::File, Source::Gelbooru, 7, None));
        let md5 = "0123456789abcdef0123456789ABCDEF";
        assert_eq!(
            parse_local(&format!("file/gelbooru/7/{md5}")),
            request(LocalRoute::File, Source::Gelbooru, 7, Some("0123456789abcdef0123456789abcdef"))
        );
        // 只有原图能带 md5，md5 必须是 32 位十六进制，后面不能再有别的。
        assert_eq!(parse_local(&format!("thumb/gelbooru/7/{md5}")), None);
        assert_eq!(parse_local("file/gelbooru/7/extra"), None);
        assert_eq!(parse_local(&format!("file/gelbooru/7/{md5}/x")), None);
        assert_eq!(parse_local("file/gelbooru/7/../../etc/passwd"), None);
        assert_eq!(parse_local("file/../../etc/passwd"), None);
        assert_eq!(parse_local("thumb/danbooru/-1"), None);
        assert_eq!(parse_local("other/danbooru/1"), None);
    }

    #[test]
    fn decodes_convert_file_src_paths() {
        // convertFileSrc 用 encodeURIComponent 编码整个远程地址。
        let path = "/https%3A%2F%2Fcdn.donmai.us%2F360x360%2F0b%2F03%2Fx.jpg";
        let raw = percent_decode_str(path.trim_start_matches('/')).decode_utf8().unwrap();
        assert_eq!(raw, "https://cdn.donmai.us/360x360/0b/03/x.jpg");
    }

    #[test]
    fn sniffs_image_formats() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff(b"\x89PNG\r\n"), Some("image/png"));
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"\x00\x00\x00\x1cftypavif"), Some("image/avif"));
        assert_eq!(sniff(b"<!DOCTYPE html>"), None);
    }
}
