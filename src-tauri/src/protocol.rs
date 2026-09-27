//! `ibx://` 图片协议：界面上的图片都经这里加载。
//!
//! - 远程图：界面不直接请求站点图床，这样代理、UA、Referer、限速和域名白名单只在一处生效；
//!   加载过的图缓存在「缓存」位置下的 remote 目录（14 天）。
//! - 图库里的图：`local/thumb/{来源}/{帖子id}` 和 `local/file/{来源}/{帖子id}`，
//!   只能读到图库登记过的文件；缩略图不存在时现场生成。
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

use crate::sources::{self, Source};
use crate::storage::StorageKind;
use crate::{thumbs, AppState};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(14 * 24 * 60 * 60);

type Failure = (StatusCode, String);

pub async fn serve<R: Runtime>(app: &AppHandle<R>, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let result = load(app, request.uri().path()).await;
    let builder = Response::builder();
    match result {
        Ok((bytes, mime)) => builder
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "private, max-age=604800")
            .body(bytes),
        Err((status, message)) => {
            #[cfg(debug_assertions)]
            eprintln!("[ibx] {} {}", status.as_u16(), message);
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
    let url = Url::parse(&raw).map_err(|_| (StatusCode::BAD_REQUEST, "图片地址无效".to_string()))?;
    let source =
        sources::source_for_url(&url).ok_or((StatusCode::FORBIDDEN, "只能加载已接入站点的图片".to_string()))?;

    // 每次请求都读取当前的缓存位置，修改位置后立即生效。
    let cache = cache_path(app, &url);
    if let Some(bytes) = read_fresh(cache.as_deref()).await {
        if let Some(mime) = sniff(&bytes) {
            return Ok((bytes, mime));
        }
    }

    let state = app.state::<AppState>();
    let request = state.net.client.get(url.clone()).header(REFERER, source.referer());
    let mut response = state.net.preview.send(request).await.map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    if !response.status().is_success() {
        return Err((StatusCode::BAD_GATEWAY, format!("{} 返回 HTTP {}", source.site_name(), response.status().as_u16())));
    }
    if response.content_length().is_some_and(|len| len as usize > MAX_BYTES) {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "图片超过 32 MB".to_string()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_BYTES {
            return Err((StatusCode::PAYLOAD_TOO_LARGE, "图片超过 32 MB".to_string()));
        }
    }
    // 以文件内容判断格式，不信任上游的 Content-Type。
    let mime = sniff(&bytes).ok_or((StatusCode::BAD_GATEWAY, "不是支持的图片格式".to_string()))?;
    if let Some(path) = cache {
        write_atomic(&path, &bytes).await;
    }
    Ok((bytes, mime))
}

#[derive(Debug, PartialEq, Eq)]
enum LocalRoute {
    Thumb,
    File,
}

fn parse_local(route: &str) -> Option<(LocalRoute, Source, u64)> {
    let mut parts = route.split('/');
    let kind = match parts.next()? {
        "thumb" => LocalRoute::Thumb,
        "file" => LocalRoute::File,
        _ => return None,
    };
    let source = Source::parse(parts.next()?)?;
    let id = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((kind, source, id))
}

async fn load_local<R: Runtime>(app: &AppHandle<R>, route: &str) -> Result<(Vec<u8>, &'static str), Failure> {
    let (kind, source, id) = parse_local(route).ok_or((StatusCode::BAD_REQUEST, "图片地址无效".to_string()))?;
    let state = app.state::<AppState>();
    let file = state
        .library
        .local_path(source, id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "图库里没有这张图".to_string()))?;
    let missing = || (StatusCode::NOT_FOUND, "图片文件不见了，可能已被移动或删除".to_string());

    if kind == LocalRoute::Thumb {
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
    }
    let bytes = tokio::fs::read(&file).await.map_err(|_| missing())?;
    let mime = sniff(&bytes).ok_or((StatusCode::UNSUPPORTED_MEDIA_TYPE, "不是支持的图片格式".to_string()))?;
    Ok((bytes, mime))
}

fn cache_path<R: Runtime>(app: &AppHandle<R>, url: &Url) -> Option<PathBuf> {
    let digest = Md5::digest(url.as_str().as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
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
    fn parses_local_routes_strictly() {
        assert_eq!(parse_local("thumb/danbooru/42"), Some((LocalRoute::Thumb, Source::Danbooru, 42)));
        assert_eq!(parse_local("file/gelbooru/7"), Some((LocalRoute::File, Source::Gelbooru, 7)));
        assert_eq!(parse_local("file/gelbooru/7/extra"), None);
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
