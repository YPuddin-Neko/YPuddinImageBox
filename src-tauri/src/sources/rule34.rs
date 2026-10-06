//! Rule34.xxx：使用兼容 Gelbooru 的 DAPI JSON 接口。
//!
//! 该站点的 API 当前要求 `user_id` 和 `api_key`，所以没有账号时不会发出搜索请求。

use reqwest::header::ACCEPT;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::{non_empty, split_tags, Post, PostTags, Rating, Source};
use crate::error::AppError;
use crate::net::Net;

const API: &str = "https://api.rule34.xxx";
const BASE: &str = "https://rule34.xxx";
const SITE: &str = "Rule34.xxx";

#[derive(Clone)]
pub struct Credentials {
    pub user_id: String,
    pub api_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("user_id", &self.user_id).field("api_key", &"***").finish()
    }
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(rename = "@attributes")]
    attributes: Option<Attributes>,
    post: Option<OneOrMany>,
}

#[derive(Debug, Deserialize)]
struct Attributes {
    #[serde(default, deserialize_with = "lenient_u64")]
    count: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    Many(Vec<RawPost>),
    One(Box<RawPost>),
}

#[derive(Debug, Deserialize)]
struct RawPost {
    #[serde(default, deserialize_with = "lenient_u64")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "lenient_u64")]
    width: Option<u64>,
    #[serde(default, deserialize_with = "lenient_u64")]
    height: Option<u64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    score: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    favorites: Option<i64>,
    hash: Option<String>,
    rating: Option<String>,
    file_url: Option<String>,
    sample_url: Option<String>,
    preview_url: Option<String>,
    image: Option<String>,
    #[serde(default, deserialize_with = "lenient_string")]
    directory: Option<String>,
    #[serde(default, deserialize_with = "lenient_u64")]
    file_size: Option<u64>,
    #[serde(default, deserialize_with = "lenient_u64")]
    filesize: Option<u64>,
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

/// 文本字段有时是数字：`directory` 实际给的是 2109 这样的数字。
fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
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

pub async fn search(net: &Net, query: &str, page: u32, limit: u32, credentials: &Credentials) -> Result<(Vec<Post>, usize), AppError> {
    parse(&index(net, query, page.saturating_sub(1), limit.min(100), credentials).await?)
}

pub async fn verify(net: &Net, credentials: &Credentials) -> Result<(), AppError> {
    parse(&index(net, "", 0, 1, credentials).await?).map(|_| ())
}

pub async fn count(net: &Net, query: &str, credentials: &Credentials) -> Result<Option<u64>, AppError> {
    let body = index(net, query, 0, 1, credentials).await?;
    let value: Value = serde_json::from_slice(&body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    if value.is_array() {
        return Ok(None);
    }
    Ok(serde_json::from_value::<Envelope>(value)
        .map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?
        .attributes
        .and_then(|attributes| attributes.count))
}

async fn index(net: &Net, query: &str, pid: u32, limit: u32, credentials: &Credentials) -> Result<Vec<u8>, AppError> {
    let request = net
        .client()
        .get(format!("{API}/index.php"))
        .query(&[
            ("page", "dapi".to_string()),
            ("s", "post".to_string()),
            ("q", "index".to_string()),
            ("json", "1".to_string()),
            ("tags", query.to_string()),
            ("pid", pid.to_string()),
            ("limit", limit.to_string()),
            ("user_id", credentials.user_id.clone()),
            ("api_key", credentials.api_key.clone()),
        ])
        .header(ACCEPT, "application/json");
    let response = net.api.send(request).await?;
    let status = response.status();
    let body = response.bytes().await?;
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::BadCredentials { site: SITE });
    }
    if !status.is_success() {
        return Err(AppError::Http { site: SITE, status: status.as_u16() });
    }
    Ok(body.to_vec())
}

pub fn parse(body: &[u8]) -> Result<(Vec<Post>, usize), AppError> {
    let value: Value = serde_json::from_slice(body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    if value.as_str().is_some_and(|message| message.to_ascii_lowercase().contains("authentication")) {
        return Err(AppError::BadCredentials { site: SITE });
    }
    let raw = if value.is_array() {
        serde_json::from_value::<Vec<RawPost>>(value)
    } else {
        let envelope = serde_json::from_value::<Envelope>(value).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
        Ok(match envelope.post {
            Some(OneOrMany::Many(posts)) => posts,
            Some(OneOrMany::One(post)) => vec![*post],
            None => Vec::new(),
        })
    }
    .map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    let count = raw.len();
    Ok((raw.into_iter().filter_map(normalize).collect(), count))
}

fn normalize(raw: RawPost) -> Option<Post> {
    let id = raw.id?;
    let file_url = non_empty(raw.file_url).or_else(|| {
        let image = raw.image.as_deref()?.trim();
        let directory = raw.directory.as_deref()?.trim_matches('/');
        (!image.is_empty() && !directory.is_empty()).then(|| format!("{BASE}/images/{directory}/{image}"))
    });
    let file_ext = file_url
        .as_deref()
        .and_then(|url| url.split('?').next()?.rsplit_once('.')?.1.split('/').next())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let width = u32::try_from(raw.width?).ok()?;
    let height = u32::try_from(raw.height?).ok()?;
    Some(Post {
        source: Source::Rule34,
        id,
        md5: non_empty(raw.hash),
        width,
        height,
        rating: raw.rating.as_deref().and_then(Rating::parse),
        score: raw.score.unwrap_or(0),
        fav_count: raw.favorites,
        file_ext,
        file_name: None,
        download_index: None,
        title: None,
        file_size: raw.file_size.or(raw.filesize),
        sample_url: non_empty(raw.sample_url).or_else(|| file_url.clone()),
        thumb_url: non_empty(raw.preview_url),
        file_url,
        created_at: raw.created_at,
        post_url: format!("{BASE}/index.php?page=post&s=view&id={id}"),
        tags: PostTags { general: split_tags(raw.tags.as_deref()), ..PostTags::default() },
        pages: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANY: &[u8] = br#"[{"id":"11235813","created_at":"2026-09-27T01:02:03-05:00","score":"12","width":"2400","height":"3400","hash":"0123456789abcdef0123456789abcdef","image":"0123456789abcdef0123456789abcdef.png","directory":"01/23","rating":"explicit","tags":"1girl scenery sky sky","file_url":"https://us.rule34.xxx/images/01/23/0123456789abcdef0123456789abcdef.png","sample_url":"","preview_url":"https://us.rule34.xxx/thumbnails/01/23/thumbnail_0123.jpg"}]"#;

    #[test]
    fn parses_json_array_and_lenient_numbers() {
        let (posts, count) = parse(MANY).unwrap();
        assert_eq!(count, 1);
        assert_eq!(posts[0].id, 11235813);
        assert_eq!(posts[0].rating, Some(Rating::Explicit));
        assert_eq!(posts[0].tags.general, vec!["1girl", "scenery", "sky"]);
        assert_eq!(posts[0].sample_url, posts[0].file_url);
    }

    /// 2026-09-29 接口实际返回的样子：数字字段都是数字，directory 也是，没有 created_at。
    const LIVE: &[u8] = br#"[{"preview_url":"https:\/\/api-cdn.rule34.xxx\/thumbnails\/2109\/thumbnail_5fe8.jpg","sample_url":"https:\/\/api-cdn.rule34.xxx\/images\/2109\/5fe8.png","file_url":"","directory":2109,"hash":"5fe824fd15219985983e75e1cbb87964","width":832,"height":1216,"id":18891869,"image":"5fe8.png","change":1790686543,"owner":"someone","parent_id":0,"rating":"explicit","sample":false,"sample_height":0,"sample_width":0,"score":1,"tags":"1girl solo","source":"","status":"active","has_notes":false,"comment_count":0}]"#;

    #[test]
    fn parses_numeric_directory() {
        let (posts, count) = parse(LIVE).unwrap();
        assert_eq!(count, 1);
        assert_eq!((posts[0].id, posts[0].width, posts[0].height), (18891869, 832, 1216));
        // 没给 file_url 时按 directory 和 image 拼出原图地址。
        assert_eq!(posts[0].file_url.as_deref(), Some("https://rule34.xxx/images/2109/5fe8.png"));
        assert_eq!(posts[0].file_ext, "png");
    }

    #[test]
    fn detects_authentication_error_body() {
        assert!(matches!(parse(br#""Missing authentication. Go to api.rule34.xxx for more information""#), Err(AppError::BadCredentials { .. })));
    }
}
