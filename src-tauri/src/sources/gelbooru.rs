//! Gelbooru：`GET /index.php?page=dapi&s=post&q=index&json=1`，每页最多 100 条，
//! 页码 `pid` 从 0 开始。接口强制要求 `user_id` + `api_key`，缺了会返回 401。
//! 帖子里的 tag 不带分类，暂时全部归入「一般」。

use reqwest::header::ACCEPT;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::{non_empty, split_tags, Post, PostTags, Rating, Source};
use crate::error::AppError;
use crate::net::Net;

const BASE: &str = "https://gelbooru.com";
const SITE: &str = "Gelbooru";

#[derive(Debug, Clone)]
pub struct Credentials {
    pub user_id: String,
    pub api_key: String,
}

#[derive(Deserialize)]
struct Envelope {
    post: Option<OneOrMany>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    Many(Vec<RawPost>),
    One(Box<RawPost>),
}

#[derive(Deserialize)]
struct RawPost {
    #[serde(default, deserialize_with = "lenient_u64")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "lenient_u64")]
    width: Option<u64>,
    #[serde(default, deserialize_with = "lenient_u64")]
    height: Option<u64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    score: Option<i64>,
    md5: Option<String>,
    rating: Option<String>,
    image: Option<String>,
    file_url: Option<String>,
    sample_url: Option<String>,
    preview_url: Option<String>,
    created_at: Option<String>,
    tags: Option<String>,
}

fn lenient_u64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

fn lenient_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

pub async fn search(
    net: &Net,
    query: &str,
    page: u32,
    limit: u32,
    credentials: &Credentials,
) -> Result<(Vec<Post>, usize), AppError> {
    let request = net
        .client
        .get(format!("{BASE}/index.php"))
        .query(&[
            ("page", "dapi".to_string()),
            ("s", "post".to_string()),
            ("q", "index".to_string()),
            ("json", "1".to_string()),
            ("tags", query.to_string()),
            ("pid", page.saturating_sub(1).to_string()),
            ("limit", limit.min(100).to_string()),
            ("user_id", credentials.user_id.clone()),
            ("api_key", credentials.api_key.clone()),
        ])
        .header(ACCEPT, "application/json");
    let response = net.api.send(request).await?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::CredentialsMissing(SITE));
    }
    if !status.is_success() {
        return Err(AppError::Http { site: SITE, status: status.as_u16() });
    }
    parse(&response.bytes().await?)
}

pub fn parse(body: &[u8]) -> Result<(Vec<Post>, usize), AppError> {
    let envelope: Envelope =
        serde_json::from_slice(body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    let raw = match envelope.post {
        Some(OneOrMany::Many(posts)) => posts,
        Some(OneOrMany::One(post)) => vec![*post],
        None => Vec::new(),
    };
    let count = raw.len();
    Ok((raw.into_iter().filter_map(normalize).collect(), count))
}

fn normalize(raw: RawPost) -> Option<Post> {
    let id = raw.id?;
    let width = u32::try_from(raw.width?).ok()?;
    let height = u32::try_from(raw.height?).ok()?;
    let file_url = non_empty(raw.file_url);
    let file_ext = raw
        .image
        .as_deref()
        .or(file_url.as_deref())
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    let sample_url = non_empty(raw.sample_url).or_else(|| file_url.clone());
    Some(Post {
        source: Source::Gelbooru,
        id,
        md5: non_empty(raw.md5),
        width,
        height,
        rating: raw.rating.as_deref().and_then(Rating::parse),
        score: raw.score.unwrap_or(0),
        fav_count: None,
        file_ext,
        file_size: None,
        file_url,
        sample_url,
        thumb_url: non_empty(raw.preview_url),
        created_at: raw.created_at,
        post_url: format!("{BASE}/index.php?page=post&s=view&id={id}"),
        tags: PostTags { general: split_tags(raw.tags.as_deref()), ..PostTags::default() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANY: &[u8] = br#"{"@attributes":{"limit":2,"offset":0,"count":5213},"post":[
      {"id":11235813,"created_at":"Sat Sep 27 01:02:03 -0500 2026","score":12,"width":2400,"height":3400,
       "md5":"0123456789abcdef0123456789abcdef","image":"0123456789abcdef0123456789abcdef.png","rating":"general",
       "tags":"1girl scenery sky  sky","file_url":"https://img4.gelbooru.com/images/01/23/0123.png",
       "sample_url":"","preview_url":"https://img4.gelbooru.com/thumbnails/01/23/thumbnail_0123.jpg","has_notes":"false"},
      {"id":"11235814","width":"1200","height":"1700","rating":"sensitive","image":"x.JPG","sample_url":"https://img4.gelbooru.com/samples/x.jpg"}
    ]}"#;

    #[test]
    fn parses_list_and_lenient_numbers() {
        let (posts, count) = parse(MANY).unwrap();
        assert_eq!(count, 2);
        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].file_ext, "png");
        assert_eq!(posts[0].tags.general, vec!["1girl", "scenery", "sky"]);
        // 没有 sample 时详情面板退回原图。
        assert_eq!(posts[0].sample_url, posts[0].file_url);
        assert_eq!(posts[1].id, 11235814);
        assert_eq!(posts[1].rating, Some(Rating::Sensitive));
        assert_eq!(posts[1].file_ext, "jpg");
    }

    #[test]
    fn parses_single_object_and_empty() {
        let (posts, _) =
            parse(br#"{"post":{"id":7,"width":10,"height":10,"image":"a.gif"}}"#).unwrap();
        assert_eq!(posts.len(), 1);
        let (posts, count) = parse(br#"{"@attributes":{"limit":100,"offset":0,"count":0}}"#).unwrap();
        assert!(posts.is_empty());
        assert_eq!(count, 0);
    }
}
