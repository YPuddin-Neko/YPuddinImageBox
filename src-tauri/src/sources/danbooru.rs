//! Danbooru：`GET /posts.json`，每页最多 200 条，账号用 HTTP Basic（用户名 + API Key）。
//! 未登录也能搜索，但速率更低、部分原图不可见。

use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

use super::{non_empty, split_tags, Page, Post, PostTags, Rating, Source};
use crate::error::AppError;
use crate::net::{user_agent, Net};

const BASE: &str = "https://danbooru.donmai.us";
const SITE: &str = "Danbooru";

#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub api_key: String,
}

/// 调试输出里不打印 API Key。
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("username", &self.username).field("api_key", &"***").finish()
    }
}

/// 账号信息，验证账号时用。
#[derive(Debug, Deserialize)]
pub struct Profile {
    pub id: Option<u64>,
    pub name: Option<String>,
    /// Member / Gold / Platinum / Builder 等。
    pub level_string: Option<String>,
}

#[derive(Deserialize)]
struct RawPost {
    id: Option<u64>,
    md5: Option<String>,
    image_width: Option<u32>,
    image_height: Option<u32>,
    rating: Option<String>,
    score: Option<i64>,
    fav_count: Option<i64>,
    file_ext: Option<String>,
    file_size: Option<u64>,
    file_url: Option<String>,
    large_file_url: Option<String>,
    preview_file_url: Option<String>,
    created_at: Option<String>,
    tag_string_artist: Option<String>,
    tag_string_copyright: Option<String>,
    tag_string_character: Option<String>,
    tag_string_general: Option<String>,
    tag_string_meta: Option<String>,
    media_asset: Option<MediaAsset>,
}

#[derive(Deserialize)]
struct MediaAsset {
    #[serde(default)]
    variants: Vec<Variant>,
}

#[derive(Deserialize)]
struct Variant {
    #[serde(rename = "type")]
    kind: String,
    url: String,
}

#[derive(Deserialize)]
struct ErrorBody {
    message: Option<String>,
}

#[derive(Deserialize)]
struct CountBody {
    counts: Counts,
}

#[derive(Deserialize)]
struct Counts {
    posts: Option<u64>,
}

/// 返回（整理后的帖子，站点这一页实际返回的条数）。后者用来判断是否还有下一页。
pub async fn search(
    net: &Net,
    query: &str,
    page: &Page,
    limit: u32,
    credentials: Option<&Credentials>,
) -> Result<(Vec<Post>, usize), AppError> {
    let request = net
        .client()
        .get(format!("{BASE}/posts.json"))
        .query(&[("tags", query.to_string()), ("page", page.to_param()), ("limit", limit.min(200).to_string())]);
    parse(&get(net, request, credentials).await?)
}

/// 查询条件的结果总数；查询太复杂时站点不给数字，返回 `None`。
/// 注意这个接口不检查 tag 数量上限，超限的查询也会返回数字。
pub async fn count(net: &Net, query: &str, credentials: Option<&Credentials>) -> Result<Option<u64>, AppError> {
    let request = net.client().get(format!("{BASE}/counts/posts.json")).query(&[("tags", query)]);
    let body = get(net, request, credentials).await?;
    let parsed: CountBody =
        serde_json::from_slice(&body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    Ok(parsed.counts.posts)
}

/// 验证账号：带账号读取 profile.json。Key 不对时站点返回 401。
pub async fn verify(net: &Net, credentials: &Credentials) -> Result<Profile, AppError> {
    let request = net.client().get(format!("{BASE}/profile.json"));
    let body = get(net, request, Some(credentials)).await?;
    let profile: Profile =
        serde_json::from_slice(&body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    // 认证没生效时站点按匿名用户返回（id 为空）。
    if profile.id.is_none() {
        return Err(AppError::BadCredentials { site: SITE });
    }
    Ok(profile)
}

async fn get(
    net: &Net,
    request: reqwest::RequestBuilder,
    credentials: Option<&Credentials>,
) -> Result<Vec<u8>, AppError> {
    let mut request = request.header(ACCEPT, "application/json");
    if let Some(creds) = credentials {
        request = request
            .basic_auth(&creds.username, Some(&creds.api_key))
            .header(USER_AGENT, user_agent(Some(&creds.username)));
    }
    let response = net.api.send(request).await?;
    let status = response.status();
    let body = response.bytes().await?;
    if !status.is_success() {
        return Err(upstream_error(status.as_u16(), &body));
    }
    Ok(body.to_vec())
}

fn upstream_error(status: u16, body: &[u8]) -> AppError {
    // 403 只有带 JSON 时才是账号问题；Cloudflare 拦截返回的是 HTML 页面。
    if status == 401 || (status == 403 && body.starts_with(b"{")) {
        return AppError::BadCredentials { site: SITE };
    }
    let message = serde_json::from_slice::<ErrorBody>(body).ok().and_then(|e| e.message);
    match message {
        // "You cannot search for more than 2 tags at a time."
        Some(msg) if msg.contains("more than") && msg.contains("tags") => {
            let limit = msg.split_whitespace().find_map(|w| w.parse::<u32>().ok()).unwrap_or(2);
            AppError::TagLimit { site: SITE, limit }
        }
        Some(msg) => AppError::Upstream { site: SITE, message: msg },
        None => AppError::Http { site: SITE, status },
    }
}

pub fn parse(body: &[u8]) -> Result<(Vec<Post>, usize), AppError> {
    let raw: Vec<RawPost> =
        serde_json::from_slice(body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    let count = raw.len();
    Ok((raw.into_iter().filter_map(normalize).collect(), count))
}

fn normalize(raw: RawPost) -> Option<Post> {
    let id = raw.id?;
    let (width, height) = (raw.image_width?, raw.image_height?);
    let variant = |kind: &str| {
        raw.media_asset
            .as_ref()
            .and_then(|asset| asset.variants.iter().find(|v| v.kind == kind))
            .map(|v| v.url.clone())
    };
    let file_url = non_empty(raw.file_url);
    let large = non_empty(raw.large_file_url);
    let thumb_url = variant("360x360").or_else(|| non_empty(raw.preview_file_url.clone()));
    let sample_url = variant("720x720").or_else(|| large.clone()).or_else(|| file_url.clone());
    Some(Post {
        source: Source::Danbooru,
        id,
        md5: non_empty(raw.md5),
        width,
        height,
        rating: raw.rating.as_deref().and_then(Rating::parse),
        score: raw.score.unwrap_or(0),
        fav_count: raw.fav_count,
        file_ext: raw.file_ext.unwrap_or_default(),
        file_name: None,
        download_index: None,
        title: None,
        file_size: raw.file_size,
        file_url,
        sample_url,
        thumb_url,
        created_at: raw.created_at,
        post_url: format!("{BASE}/posts/{id}"),
        tags: PostTags {
            artist: split_tags(raw.tag_string_artist.as_deref()),
            copyright: split_tags(raw.tag_string_copyright.as_deref()),
            character: split_tags(raw.tag_string_character.as_deref()),
            general: split_tags(raw.tag_string_general.as_deref()),
            meta: split_tags(raw.tag_string_meta.as_deref()),
        },
        pages: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_with_variants() {
        let (posts, count) = parse(include_bytes!("../../tests/fixtures/danbooru_posts.json")).unwrap();
        assert_eq!(count, 2);
        assert_eq!(posts.len(), 2);
        let post = &posts[0];
        assert_eq!(post.id, 12266257);
        assert_eq!(post.rating, Some(Rating::General));
        assert_eq!((post.width, post.height), (800, 600));
        assert!(post.thumb_url.as_deref().unwrap().contains("/360x360/"));
        assert!(post.sample_url.as_deref().unwrap().contains("/720x720/"));
        assert_eq!(post.tags.copyright, vec!["divine_heart_makina".to_string()]);
        assert!(post.tags.general.contains(&"scenery".to_string()));
        assert_eq!(post.post_url, "https://danbooru.donmai.us/posts/12266257");
    }

    #[test]
    fn maps_tag_limit_message() {
        let body = br#"{"success":false,"message":"You cannot search for more than 2 tags at a time."}"#;
        match upstream_error(422, body) {
            AppError::TagLimit { limit, .. } => assert_eq!(limit, 2),
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(upstream_error(500, b"<html>"), AppError::Http { status: 500, .. }));
        let bad_key = br#"{"success":false,"error":"SessionLoader::AuthenticationFailure","message":"Invalid API key"}"#;
        assert!(matches!(upstream_error(401, bad_key), AppError::BadCredentials { .. }));
        // Cloudflare 拦截页也是 403，但不是账号问题。
        assert!(matches!(upstream_error(403, b"<!DOCTYPE html>"), AppError::Http { status: 403, .. }));
    }

    #[test]
    fn debug_output_hides_api_key() {
        let creds = Credentials { username: "sora".into(), api_key: "secret-key".into() };
        let printed = format!("{creds:?}");
        assert!(printed.contains("sora") && !printed.contains("secret-key"));
    }

    #[test]
    fn parses_count_body() {
        let parsed: CountBody = serde_json::from_slice(br#"{"counts":{"posts":66491}}"#).unwrap();
        assert_eq!(parsed.counts.posts, Some(66491));
        // 带 filesize 等开销大的条件时站点返回 null。
        let parsed: CountBody = serde_json::from_slice(br#"{"counts":{"posts":null}}"#).unwrap();
        assert_eq!(parsed.counts.posts, None);
    }

    #[test]
    fn skips_posts_without_dimensions() {
        let (posts, count) = parse(br#"[{"id":1},{"id":2,"image_width":10,"image_height":20}]"#).unwrap();
        assert_eq!(count, 2);
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].id, 2);
    }
}
