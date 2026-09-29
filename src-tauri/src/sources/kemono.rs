//! Kemono：使用官方 API v1 读取图片附件。
//!
//! 搜索框支持两种形式：普通文本使用全站 `/v1/posts` 搜索；指定作者使用
//! `creator:服务/作者ID`，例如 `creator:patreon/123456`。每个帖子里的图片文件和附件
//! 会分别变成一张可预览、可下载的图片，视频、压缩包和其他文件会跳过。

use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

use super::{Page, Post, PostTags, Source};
use crate::error::AppError;
use crate::i18n::tr;
use crate::net::{user_agent, Net};

const BASE: &str = "https://kemono.cr";
const SITE: &str = "Kemono";
const PAGE_SIZE: u32 = 50;
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

pub async fn count(net: &Net, query: &str) -> Result<Option<u64>, AppError> {
    let query = parse_query(query)?;
    let body = request(net, &query, 0).await?;
    let page: PageEnvelope = serde_json::from_slice(&body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    Ok(page.true_count.or(Some(page.count as u64)))
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
    let response = net.api.send(request).await?;
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

pub fn parse(body: &[u8]) -> Result<(Vec<Post>, usize), AppError> {
    let page: PageEnvelope = serde_json::from_slice(body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    let count = page.posts.len();
    let posts = page.posts.into_iter().flat_map(normalize).collect();
    Ok((posts, count))
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
    files
        .into_iter()
        .filter_map(|file| {
            let path = file.path.as_deref()?.trim().replace('\\', "/");
            let url = file_url(&path)?;
            let ext = extension(file.name.as_deref()).or_else(|| extension(Some(&path)))?;
            if !IMAGE_EXTS.contains(&ext.as_str()) || !seen.insert(url.clone()) {
                return None;
            }
            Some((url, ext))
        })
        .enumerate()
        .filter_map(|(index, (url, ext))| {
            let id = post_id.checked_mul(1000)?.checked_add(index as u64);
            let id = id?;
            Some(Post {
                source: Source::Kemono,
                id,
                md5: None,
                width: 1,
                height: 1,
                rating: None,
                score: 0,
                fav_count: None,
                file_ext: ext,
                file_size: None,
                file_url: Some(url.clone()),
                sample_url: Some(url.clone()),
                thumb_url: Some(url),
                created_at: raw.published.clone(),
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
        assert_eq!(posts[0].id, 123_000);
        assert_eq!(posts[0].file_url.as_deref(), Some("https://kemono.cr/data/aa/bb/one.jpg"));
        assert_eq!(posts[0].tags.artist, vec!["patreon:456"]);
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
}
