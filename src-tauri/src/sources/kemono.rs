//! Kemono：使用官方 API v1 读取图片附件。
//!
//! 搜索框支持两种形式：普通文本使用全站 `/v1/posts` 搜索；指定作者使用
//! `creator:服务/作者ID`，例如 `creator:patreon/123456`。每个帖子里的图片文件和附件
//! 会分别变成一张可预览、可下载的图片，视频、压缩包和其他文件会跳过。
//!
//! 接口不给图片尺寸，宽高记为 1 × 1 表示未知。

use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

use super::{timestamp, Page, Post, PostTags, Source};
use crate::error::AppError;
use crate::i18n::tr;
use crate::net::{user_agent, Net};

const BASE: &str = "https://kemono.cr";
const SITE: &str = "Kemono";
const PAGE_SIZE: u32 = 50;
/// 预览用的缩略图，长边不超过 800。原图地址会跳转到 n1–n4 数据节点，那几台经常连不上，图也太大。
const THUMB_BASE: &str = "https://img.kemono.cr/thumbnail/data";
/// 编号 = 服务序号 × 10¹³ + 帖子 id × 1000 + 第几张。各服务的帖子 id 各自编号，不加服务会撞号；
/// 界面和文件名只显示帖子 id 和第几张。
const SERVICE_FACTOR: u64 = 10_000_000_000_000;
const MAX_FILES: usize = 1000;
const SERVICES: [&str; 10] = [
    "patreon",
    "fanbox",
    "fantia",
    "discord",
    "afdian",
    "boosty",
    "dlsite",
    "gumroad",
    "subscribestar",
    "onlyfans",
];
const IMAGE_EXTS: [&str; 6] = ["jpg", "jpeg", "png", "gif", "webp", "avif"];

#[derive(Debug, Deserialize)]
struct PageEnvelope {
    #[serde(default)]
    count: usize,
    #[serde(default)]
    true_count: Option<u64>,
    #[serde(default)]
    posts: Vec<RawPost>,
}

#[derive(Debug, Deserialize)]
struct RawPost {
    #[serde(deserialize_with = "string_value")]
    id: String,
    #[serde(deserialize_with = "string_value")]
    user: String,
    #[serde(deserialize_with = "string_value")]
    service: String,
    published: Option<String>,
    file: Option<RawFile>,
    #[serde(default)]
    attachments: Vec<RawFile>,
}

#[derive(Debug, Deserialize, Clone)]
struct RawFile {
    name: Option<String>,
    path: Option<String>,
}

fn string_value<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(value) => value,
        serde_json::Value::Number(value) => value.to_string(),
        _ => String::new(),
    })
}

#[derive(Debug)]
struct Query {
    service: Option<String>,
    creator: Option<String>,
    text: String,
    tag: Option<String>,
}

pub async fn search(net: &Net, query: &str, page: &Page, _limit: u32) -> Result<(Vec<Post>, usize), AppError> {
    let query = parse_query(query)?;
    let offset = match page {
        Page::Number(number) => number.saturating_sub(1).saturating_mul(PAGE_SIZE),
        // Kemono 的列表接口只支持 offset；订阅不会使用这两个方向的游标。
        Page::Before(_) | Page::After(_) => 0,
    };
    let body = request(net, &query, offset).await?;
    parse(&body)
}

/// 帖子数，不是图片数：一个帖子可能有好几张图。
pub async fn count(net: &Net, query: &str) -> Result<Option<u64>, AppError> {
    let query = parse_query(query)?;
    let body = request(net, &query, 0).await?;
    Ok(listing(&body)?.1)
}

async fn request(net: &Net, query: &Query, offset: u32) -> Result<Vec<u8>, AppError> {
    let endpoint = match (&query.service, &query.creator) {
        (Some(service), Some(creator)) => format!("{BASE}/api/v1/{service}/user/{creator}/posts"),
        _ => format!("{BASE}/api/v1/posts"),
    };
    let mut params = vec![("o", offset.to_string())];
    if !query.text.is_empty() {
        params.push(("q", query.text.clone()));
    }
    if let Some(tag) = &query.tag {
        params.push(("tag", tag.clone()));
    }
    let request = net
        .client()
        .get(endpoint)
        .query(&params)
        .header(ACCEPT, "text/css")
        .header(USER_AGENT, user_agent(None));
    let response = net.kemono.send(request).await?;
    let status = response.status();
    let body = response.bytes().await?;
    if !status.is_success() {
        return Err(AppError::Http { site: SITE, status: status.as_u16() });
    }
    Ok(body.to_vec())
}

fn parse_query(value: &str) -> Result<Query, AppError> {
    let mut service = None;
    let mut creator = None;
    let mut text = Vec::new();
    let mut tags = Vec::new();
    for token in value.split_whitespace() {
        if let Some(value) = token.strip_prefix("tag:").filter(|value| !value.is_empty()) {
            tags.push(value.to_string());
            continue;
        }
        if let Some(value) = token.strip_prefix("creator:").or_else(|| token.strip_prefix("artist:")).or_else(|| token.strip_prefix("user:")) {
            let (found_service, found_creator) = split_creator(value)?;
            service = Some(found_service);
            creator = Some(found_creator);
            continue;
        }
        if let Some((maybe_service, maybe_creator)) = token.split_once(':') {
            if SERVICES.contains(&maybe_service.to_ascii_lowercase().as_str()) && !maybe_creator.is_empty() {
                service = Some(maybe_service.to_ascii_lowercase());
                creator = Some(maybe_creator.to_string());
                continue;
            }
        }
        if let Some((maybe_service, maybe_creator)) = token.split_once('/') {
            if SERVICES.contains(&maybe_service.to_ascii_lowercase().as_str()) && !maybe_creator.is_empty() {
                service = Some(maybe_service.to_ascii_lowercase());
                creator = Some(maybe_creator.to_string());
                continue;
            }
        }
        text.push(token.to_string());
    }
    if service.is_some() != creator.is_some() {
        return Err(AppError::InvalidInput(tr!("Kemono 的作者条件要写成 creator:服务/作者 ID", "Kemono creator filters use creator:service/creator ID")));
    }
    Ok(Query { service, creator, text: text.join(" "), tag: (!tags.is_empty()).then(|| tags.join(" ")) })
}

fn split_creator(value: &str) -> Result<(String, String), AppError> {
    let value = value.trim_matches('/');
    let (service, creator) = value.split_once('/').or_else(|| value.split_once(':')).ok_or_else(|| {
        AppError::InvalidInput(tr!("Kemono 的作者条件要写成 creator:服务/作者 ID", "Kemono creator filters use creator:service/creator ID"))
    })?;
    let service = service.to_ascii_lowercase();
    if !SERVICES.contains(&service.as_str()) || creator.trim().is_empty() {
        return Err(AppError::InvalidInput(tr!("Kemono 的服务或作者 ID 不正确", "The Kemono service or creator ID is invalid")));
    }
    Ok((service, creator.trim().to_string()))
}

/// 全站搜索返回 `{count, true_count, posts}`，作者的帖子列表直接返回数组（没有总数）。
fn listing(body: &[u8]) -> Result<(Vec<RawPost>, Option<u64>), AppError> {
    let parse_error = |e: serde_json::Error| AppError::Parse { site: SITE, detail: e.to_string() };
    if body.trim_ascii_start().first() == Some(&b'[') {
        return Ok((serde_json::from_slice(body).map_err(parse_error)?, None));
    }
    let page: PageEnvelope = serde_json::from_slice(body).map_err(parse_error)?;
    Ok((page.posts, page.true_count.or(Some(page.count as u64))))
}

pub fn parse(body: &[u8]) -> Result<(Vec<Post>, usize), AppError> {
    let (posts, _) = listing(body)?;
    let count = posts.len();
    Ok((posts.into_iter().flat_map(normalize).collect(), count))
}

/// 编号里的帖子 id 和第几张（从 0 开始）。
pub fn split_id(id: u64) -> (u64, u64) {
    (id % SERVICE_FACTOR / 1000, id % 1000)
}

fn normalize(raw: RawPost) -> Vec<Post> {
    let post_id = match raw.id.parse::<u64>() {
        Ok(id) => id,
        Err(_) => return Vec::new(),
    };
    let mut files = Vec::new();
    if let Some(file) = raw.file {
        files.push(file);
    }
    files.extend(raw.attachments);
    let mut seen = std::collections::HashSet::new();
    let creator_tag = format!("{}:{}", raw.service, raw.user);
    let slot = SERVICES.iter().position(|service| *service == raw.service).map_or(0, |index| index as u64 + 1);
    // 接口给的是不带时区的 UTC 时间。写成带 Z 的格式，不然界面会当成本地时间读，东八区会早一天。
    let created_at = raw
        .published
        .as_deref()
        .and_then(timestamp::parse)
        .map(|ms| timestamp::iso_utc(ms.div_euclid(1000)))
        .or_else(|| raw.published.clone());
    files
        .into_iter()
        .filter_map(|file| {
            let path = file.path.as_deref()?.trim().replace('\\', "/");
            let url = file_url(&path)?;
            let ext = extension(file.name.as_deref()).or_else(|| extension(Some(&path)))?;
            if !IMAGE_EXTS.contains(&ext.as_str()) || !seen.insert(url.clone()) {
                return None;
            }
            let thumb = thumb_url(&path).unwrap_or_else(|| url.clone());
            Some((url, thumb, ext))
        })
        .take(MAX_FILES)
        .enumerate()
        .filter_map(|(index, (url, thumb, ext))| {
            let id = post_id.checked_mul(1000)?.checked_add(index as u64).filter(|id| *id < SERVICE_FACTOR)?;
            Some(Post {
                source: Source::Kemono,
                id: slot * SERVICE_FACTOR + id,
                md5: None,
                width: 1,
                height: 1,
                rating: None,
                score: 0,
                fav_count: None,
                file_ext: ext,
                file_size: None,
                file_url: Some(url),
                sample_url: Some(thumb.clone()),
                thumb_url: Some(thumb),
                created_at: created_at.clone(),
                post_url: format!("{BASE}/{}/user/{}/post/{}#file-{}", raw.service, raw.user, raw.id, index + 1),
                tags: PostTags { artist: vec![creator_tag.clone()], ..PostTags::default() },
                pages: None,
            })
        })
        .collect()
}

fn file_url(path: &str) -> Option<String> {
    if path.starts_with("https://") || path.starts_with("http://") {
        return Some(path.to_string());
    }
    path.strip_prefix('/').map(|path| format!("{BASE}/data/{path}"))
}

fn thumb_url(path: &str) -> Option<String> {
    path.strip_prefix('/').map(|path| format!("{THUMB_BASE}/{path}"))
}

fn extension(value: Option<&str>) -> Option<String> {
    let path = value?.split('?').next()?.rsplit_once('.')?.1;
    let ext = path.split('/').next()?.to_ascii_lowercase();
    IMAGE_EXTS.contains(&ext.as_str()).then_some(ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = br#"{"count":2,"true_count":2,"posts":[
      {"id":"123","user":"456","service":"patreon","title":"art","published":"2026-09-28T00:00:00","file":{"name":"clip.mp4","path":"/aa/bb/video.mp4"},"attachments":[{"name":"one.jpg","path":"/aa/bb/one.jpg"},{"name":"one.jpg","path":"/aa/bb/one.jpg"},{"name":"pack.zip","path":"/aa/bb/pack.zip"}]},
      {"id":"124","user":"456","service":"patreon","title":"text","published":"2026-09-27T00:00:00","file":{},"attachments":[]}
    ]}"#;

    #[test]
    fn parses_image_attachments_and_skips_other_files() {
        let (posts, count) = parse(BODY).unwrap();
        assert_eq!(count, 2);
        assert_eq!(posts.len(), 1);
        assert_eq!(split_id(posts[0].id), (123, 0));
        assert_eq!(posts[0].file_url.as_deref(), Some("https://kemono.cr/data/aa/bb/one.jpg"));
        assert_eq!(posts[0].thumb_url.as_deref(), Some("https://img.kemono.cr/thumbnail/data/aa/bb/one.jpg"));
        assert_eq!(posts[0].sample_url, posts[0].thumb_url);
        assert_eq!(posts[0].created_at.as_deref(), Some("2026-09-28T00:00:00Z"));
        assert_eq!(posts[0].tags.artist, vec!["patreon:456"]);
    }

    #[test]
    fn parses_creator_listing_returned_as_array() {
        let body = br#"[{"id":"7","user":"9","service":"fanbox","published":"2022-12-11T07:36:10","file":{"name":"a.png","path":"/cc/dd/a.png"},"attachments":[{"name":"b.jpg","path":"/cc/dd/b.jpg"}]}]"#;
        let (posts, count) = parse(body).unwrap();
        assert_eq!(count, 1);
        assert_eq!(posts.iter().map(|post| split_id(post.id)).collect::<Vec<_>>(), vec![(7, 0), (7, 1)]);
        assert_eq!(posts[0].created_at.as_deref(), Some("2022-12-11T07:36:10Z"));
    }

    #[test]
    fn same_post_id_on_different_services_gets_different_ids() {
        let body = br#"[{"id":"7","user":"9","service":"fanbox","file":{"path":"/a/b/x.jpg"}},{"id":"7","user":"9","service":"fantia","file":{"path":"/a/b/y.jpg"}}]"#;
        let (posts, _) = parse(body).unwrap();
        assert_ne!(posts[0].id, posts[1].id);
        assert_eq!(split_id(posts[0].id), split_id(posts[1].id));
    }

    #[test]
    fn parses_creator_and_tag_conditions() {
        let query = parse_query("creator:patreon/456 tag:illustration summer").unwrap();
        assert_eq!(query.service.as_deref(), Some("patreon"));
        assert_eq!(query.creator.as_deref(), Some("456"));
        assert_eq!(query.tag.as_deref(), Some("illustration"));
        assert_eq!(query.text, "summer");
        assert!(parse_query("creator:unknown/456").is_err());
    }

    /// 真实网络：全站搜索和作者帖子列表（两种返回格式）都能解析；缩略图照界面加载图片的方式
    /// （域名白名单、Referer、预览通道）直接取到图片，不会跳转到连不上的数据节点。
    #[tokio::test]
    #[ignore = "需要网络，手动运行"]
    async fn searches_kemono() {
        use reqwest::header::REFERER;
        let net = Net::new(&crate::settings::ProxySettings::default()).unwrap();
        let (posts, fetched) = search(&net, "G4ku", &Page::Number(1), 50).await.unwrap();
        assert!(fetched > 0 && !posts.is_empty());
        assert!(posts.iter().all(|p| p.thumb_url.as_deref().is_some_and(|url| url.starts_with(THUMB_BASE))));
        assert!(posts.iter().all(|p| p.created_at.as_deref().is_some_and(|time| time.ends_with('Z'))));
        let total = count(&net, "G4ku").await.unwrap();
        println!("G4ku：{} 个帖子里 {} 张图，接口总数 {total:?}", fetched, posts.len());
        assert!(total.is_some());

        let (creator, fetched) = search(&net, "creator:fanbox/237082", &Page::Number(1), 50).await.unwrap();
        println!("fanbox/237082：{fetched} 个帖子里 {} 张图", creator.len());
        assert!(!creator.is_empty());
        assert_eq!(count(&net, "creator:fanbox/237082").await.unwrap(), None);

        let thumb = url::Url::parse(posts[0].thumb_url.as_deref().unwrap()).unwrap();
        assert_eq!(super::super::source_for_url(&thumb), Some(Source::Kemono));
        let request = net.client().get(thumb.clone()).header(REFERER, Source::Kemono.referer());
        let response = net.preview.send(request).await.unwrap();
        assert!(response.status().is_success(), "{} {}", response.status(), thumb);
        let bytes = response.bytes().await.unwrap();
        assert!(crate::protocol::sniff(&bytes).is_some(), "{thumb} 返回的不是图片");
    }
}
