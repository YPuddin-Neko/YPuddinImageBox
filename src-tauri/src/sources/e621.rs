//! e621：兼容 Danbooru 风格的 JSON API。
//!
//! 接口要求客户端使用有辨识度的 User-Agent；未登录也能搜索公开帖子，账号用于提高接口权限和访问受限文件。

use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;

use super::{non_empty, Page, Post, PostTags, Rating, Source};
use crate::error::AppError;
use crate::net::{user_agent, Net};

const BASE: &str = "https://e621.net";
const SITE: &str = "e621";

#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub api_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("username", &self.username).field("api_key", &"***").finish()
    }
}

#[derive(Debug, Deserialize)]
struct PostsEnvelope {
    #[serde(default)]
    posts: Vec<RawPost>,
}

#[derive(Debug, Deserialize)]
struct CountBody {
    count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawPost {
    id: Option<u64>,
    created_at: Option<String>,
    rating: Option<String>,
    score: Option<Score>,
    fav_count: Option<i64>,
    file: Option<FileInfo>,
    preview: Option<FileInfo>,
    sample: Option<SampleInfo>,
    tags: Option<RawTags>,
}

#[derive(Debug, Deserialize)]
struct Score {
    total: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct FileInfo {
    url: Option<String>,
    md5: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    size: Option<u64>,
    ext: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SampleInfo {
    #[serde(default)]
    has: bool,
    url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTags {
    #[serde(default)]
    general: Vec<String>,
    #[serde(default)]
    artist: Vec<String>,
    #[serde(default)]
    copyright: Vec<String>,
    #[serde(default)]
    character: Vec<String>,
    #[serde(default)]
    contributor: Vec<String>,
    #[serde(default)]
    species: Vec<String>,
    #[serde(default)]
    invalid: Vec<String>,
    #[serde(default)]
    meta: Vec<String>,
    #[serde(default)]
    lore: Vec<String>,
}

/// 收藏页用的条件（不是站点的语法）：自己的收藏，按收藏时间新的在前，要登录。
pub const FAVORITES: &str = "favorites:";

fn is_favorites(query: &str) -> bool {
    query.split_whitespace().any(|tag| tag == FAVORITES)
}

pub async fn search(
    net: &Net,
    query: &str,
    page: &Page,
    limit: u32,
    credentials: Option<&Credentials>,
) -> Result<(Vec<Post>, usize), AppError> {
    let limit = limit.min(320).to_string();
    let request = if is_favorites(query) {
        // 站点搜索的 fav: 只能按帖子 id 排；/favorites.json 按收藏时间排，不带 user_id 时就是自己的收藏。
        credentials.ok_or(AppError::FavoritesSignIn(SITE))?;
        net.client().get(format!("{BASE}/favorites.json")).query(&[("page", page.to_param()), ("limit", limit)])
    } else {
        net.client().get(format!("{BASE}/posts.json")).query(&[("tags", query.to_string()), ("page", page.to_param()), ("limit", limit)])
    };
    let body = get(net, request, credentials).await?;
    parse(&body)
}

pub async fn count(net: &Net, query: &str, credentials: Option<&Credentials>) -> Result<Option<u64>, AppError> {
    // 收藏的总数用 fav:用户名 统计，和按收藏时间列出来的是同一批帖子。
    let query = match credentials {
        Some(credentials) if is_favorites(query) => format!("fav:{}", credentials.username),
        None if is_favorites(query) => return Err(AppError::FavoritesSignIn(SITE)),
        _ => query.to_string(),
    };
    let request = net.client().get(format!("{BASE}/posts/count.json")).query(&[("tags", query)]);
    let body = get(net, request, credentials).await?;
    let parsed: CountBody = serde_json::from_slice(&body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    Ok(parsed.count)
}

/// 验证账号：读取一条公开帖子；账号或 Key 不对时接口返回 401/403。
pub async fn verify(net: &Net, credentials: &Credentials) -> Result<(), AppError> {
    let request = net.client().get(format!("{BASE}/posts.json")).query(&[("limit", "1")]);
    get(net, request, Some(credentials)).await.map(|_| ())
}

async fn get(
    net: &Net,
    request: reqwest::RequestBuilder,
    credentials: Option<&Credentials>,
) -> Result<Vec<u8>, AppError> {
    let mut request = request.header(ACCEPT, "application/json");
    if let Some(credentials) = credentials {
        request = request
            .basic_auth(&credentials.username, Some(&credentials.api_key))
            .header(USER_AGENT, user_agent(Some(&credentials.username)));
    }
    let response = net.api.send(request).await?;
    let status = response.status();
    let body = response.bytes().await?;
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError::BadCredentials { site: SITE });
    }
    if !status.is_success() {
        return Err(AppError::Http { site: SITE, status: status.as_u16() });
    }
    Ok(body.to_vec())
}

pub fn parse(body: &[u8]) -> Result<(Vec<Post>, usize), AppError> {
    let envelope: PostsEnvelope = serde_json::from_slice(body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    let count = envelope.posts.len();
    Ok((envelope.posts.into_iter().filter_map(normalize).collect(), count))
}

fn normalize(raw: RawPost) -> Option<Post> {
    let id = raw.id?;
    let file = raw.file?;
    let file_url = non_empty(file.url);
    let (width, height) = (file.width?, file.height?);
    let sample_url = raw.sample.filter(|sample| sample.has).and_then(|sample| non_empty(sample.url)).or_else(|| file_url.clone());
    let thumb_url = raw.preview.and_then(|preview| non_empty(preview.url));
    let tags = raw.tags.unwrap_or_default();
    let mut meta = tags.meta;
    meta.extend(tags.species);
    meta.extend(tags.lore);
    meta.extend(tags.contributor);
    meta.extend(tags.invalid);
    Some(Post {
        source: Source::E621,
        id,
        md5: non_empty(file.md5),
        width,
        height,
        rating: raw.rating.as_deref().and_then(parse_rating),
        score: raw.score.and_then(|score| score.total).unwrap_or(0),
        fav_count: raw.fav_count,
        file_ext: file.ext.unwrap_or_default().to_ascii_lowercase(),
        file_name: None,
        title: None,
        file_size: file.size,
        file_url,
        sample_url,
        thumb_url,
        created_at: raw.created_at,
        post_url: format!("{BASE}/posts/{id}"),
        tags: PostTags { artist: tags.artist, copyright: tags.copyright, character: tags.character, general: tags.general, meta },
        pages: None,
    })
}

fn parse_rating(value: &str) -> Option<Rating> {
    match value.trim().to_ascii_lowercase().as_str() {
        "s" | "safe" => Some(Rating::General),
        "q" | "questionable" => Some(Rating::Questionable),
        "e" | "explicit" => Some(Rating::Explicit),
        _ => None,
    }
}

/// e621 只有 safe / questionable / explicit 三档；界面的「敏感」按 questionable 查询。
pub fn rating_terms(ratings: &[Rating]) -> Vec<String> {
    let mut codes = Vec::new();
    for rating in ratings {
        let code = match rating {
            Rating::General => "s",
            Rating::Sensitive | Rating::Questionable => "q",
            Rating::Explicit => "e",
        };
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    if codes.is_empty() { Vec::new() } else { vec![format!("rating:{}", codes.join(","))] }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = br#"{"posts":[
      {"id":39,"created_at":"2007-02-10T15:24:17.878-05:00","file":{"width":415,"height":535,"ext":"jpg","size":28826,"url":"https://static1.e621.net/data/50/ba/a.jpg"},
       "preview":{"url":"https://static1.e621.net/data/preview/50/ba/a.jpg"},"sample":{"has":false,"url":null},
       "score":{"total":106},"fav_count":107,"rating":"s","tags":{"general":["anthro"],"artist":["kurobai"],"species":["feline"],"lore":[],"meta":[]}},
      {"id":40,"created_at":"2026-01-01T00:00:00Z","file":{"width":100,"height":200,"ext":"png","size":1000,"url":"https://static1.e621.net/data/b.png"},"rating":"e","tags":{"general":[],"artist":[],"copyright":[],"character":[],"meta":[]}}
    ]}"#;

    #[test]
    fn parses_envelope_and_maps_e621_categories() {
        let (posts, count) = parse(BODY).unwrap();
        assert_eq!(count, 2);
        assert_eq!(posts[0].rating, Some(Rating::General));
        assert_eq!(posts[0].score, 106);
        assert_eq!(posts[0].tags.meta, vec!["feline"]);
        assert_eq!(posts[1].rating, Some(Rating::Explicit));
    }

    #[test]
    fn maps_rating_filter() {
        assert_eq!(rating_terms(&[Rating::General]), vec!["rating:s"]);
        assert_eq!(rating_terms(&[Rating::Sensitive, Rating::Questionable, Rating::Explicit]), vec!["rating:q,e"]);
    }
}
