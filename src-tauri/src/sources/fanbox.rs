//! FANBOX 的作者投稿、图片与附件。每项资源独立入库，付费权限由站点当前会话决定。

use std::collections::{BTreeSet, HashMap, HashSet};
use sha2::{Digest, Sha256};

use reqwest::header::{HeaderValue, ACCEPT, COOKIE, ORIGIN, REFERER, USER_AGENT};
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::{Page, Post, PostTags, Rating, Source};
use crate::error::AppError;
use crate::i18n::tr;
use crate::net::{self, Net};

const SITE: &str = "FANBOX";
const API_HOST: &str = "api.fanbox.cc";
const MEDIA_HOST: &str = "downloads.fanbox.cc";
pub const REFERER_URL: &str = "https://www.fanbox.cc/";
pub const PAGE_SIZE: u32 = 10;
const PAGE_FACTOR: u64 = 1000;
const COVER_INDEX: usize = 999;
const COVER_CROP: &str = "/c/1200x630_90_a2_g5";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36";

#[derive(Clone)]
pub struct Credentials {
    pub session: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("session", &"***")
            .finish()
    }
}

impl Credentials {
    pub fn from_session(value: &str) -> Option<Self> {
        let value = value.trim();
        let session = value.strip_prefix("FANBOXSESSID=").unwrap_or(value).trim();
        valid_session(session).then(|| Self {
            session: session.into(),
        })
    }
}

fn valid_session(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4096
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b';' | b',' | b'"' | b'\\'))
}

pub fn split_id(id: u64) -> (u64, u32) {
    (id / PAGE_FACTOR, (id % PAGE_FACTOR) as u32)
}

fn image_id(post: u64, index: usize) -> Result<u64, AppError> {
    if post == 0 || index >= PAGE_FACTOR as usize {
        return Err(parse_error("post/image ID is out of range"));
    }
    post.checked_mul(PAGE_FACTOR)
        .and_then(|id| id.checked_add(index as u64))
        .filter(|id| *id <= MAX_SAFE_INTEGER)
        .ok_or_else(|| parse_error("post/image ID exceeds the supported range"))
}

fn parse_error(detail: impl Into<String>) -> AppError {
    AppError::Parse {
        site: SITE,
        detail: detail.into(),
    }
}

fn restricted() -> AppError {
    AppError::Upstream {
        site: SITE,
        message: tr!(
            "当前账号无权查看这篇投稿的正文",
            "The current account cannot access this post's content"
        ),
    }
}

fn trusted_url(url: &Url, host: &str) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn cover_url(raw: &str, post_id: u64) -> Option<Url> {
    let mut url = Url::parse(raw).ok()?;
    if post_id == 0 || !trusted_url(&url, "pixiv.pximg.net") {
        return None;
    }
    let path = url.path().strip_prefix(COVER_CROP).unwrap_or(url.path());
    let prefix = format!("/fanbox/public/images/post/{post_id}/cover/");
    let file = path.strip_prefix(&prefix)?;
    let (name, extension) = file.rsplit_once('.')?;
    if name.is_empty()
        || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || !matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif")
    {
        return None;
    }
    let original_path = path.to_string();
    url.set_path(&original_path);
    Some(url)
}

/// 旧版编号携带投稿号，新版资源编号只标识资源；投稿号从站点链接读取。
pub fn post_id(post: &Post) -> u64 {
    target_of(&post.post_url).and_then(|target| match target { Target::Post(id) => Some(id), _ => None })
        .unwrap_or_else(|| split_id(post.id).0)
}

pub fn display_index(post: &Post) -> u32 {
    post.download_index.unwrap_or_else(|| if is_cover(post) { 0 } else { split_id(post.id).1 + 1 })
}

pub fn is_cover(post: &Post) -> bool {
    post.source == Source::Fanbox
        && (post.id >= (1u64 << 62) || split_id(post.id).1 == COVER_INDEX as u32)
        && post.file_url.as_deref().and_then(|url| cover_url(url, post_id(post))).is_some()
}

pub fn legacy_resource_id(post: &Post) -> Result<u64, AppError> {
    resource_id(post_id(post), "legacy-url", post.file_url.as_deref().ok_or_else(|| parse_error("missing resource URL"))?)
}

fn resource_id(post: u64, kind: &str, key: &str) -> Result<u64, AppError> {
    if key.is_empty() { return Err(parse_error("missing resource ID")); }
    let digest = Sha256::digest(format!("fanbox:{post}:{kind}:{key}").as_bytes());
    let hash = u64::from_be_bytes(digest[..8].try_into().expect("SHA-256 prefix"));
    Ok((1u64 << 62) | (hash & ((1u64 << 62) - 1)))
}

fn request(
    net: &Net,
    url: Url,
    credentials: Option<&Credentials>,
) -> Result<RequestBuilder, AppError> {
    let mut request = net
        .fanbox_client()
        .get(url)
        .header(USER_AGENT, BROWSER_UA)
        .header(REFERER, REFERER_URL)
        .header(ORIGIN, "https://www.fanbox.cc");
    if let Some(credentials) = credentials {
        if !valid_session(&credentials.session) {
            return Err(AppError::BadCredentials { site: SITE });
        }
        let mut cookie = HeaderValue::from_str(&format!("FANBOXSESSID={}", credentials.session))
            .map_err(|_| AppError::BadCredentials { site: SITE })?;
        cookie.set_sensitive(true);
        request = request.header(COOKIE, cookie);
    }
    Ok(request)
}

/// 只把 FANBOX 会话发给正文下载域名；公开头像和封面无需会话。
pub fn media_request(
    net: &Net,
    url: Url,
    credentials: Option<&Credentials>,
) -> Result<RequestBuilder, AppError> {
    if trusted_url(&url, MEDIA_HOST) {
        request(net, url, credentials)
    } else if trusted_url(&url, "pixiv.pximg.net") {
        request(net, url, None)
    } else {
        Err(AppError::InvalidInput(tr!(
            "FANBOX 图片地址无效",
            "Invalid FANBOX image URL"
        )))
    }
}

fn api_url(endpoint: &str, params: &[(&str, &str)]) -> Url {
    let mut url =
        Url::parse(&format!("https://{API_HOST}/{endpoint}")).expect("fixed FANBOX API URL");
    url.query_pairs_mut().extend_pairs(params.iter().copied());
    url
}

async fn get(net: &Net, credentials: Option<&Credentials>, url: Url) -> Result<Value, AppError> {
    if !trusted_url(&url, API_HOST) {
        return Err(parse_error("untrusted API URL"));
    }
    let request = request(net, url, credentials)?
        .header(ACCEPT, "application/json")
        .header("Sec-Fetch-Dest", "empty")
        .header("Sec-Fetch-Mode", "cors")
        .header("Sec-Fetch-Site", "same-site");
    let response = net.fanbox.send(request).await?;
    if net::challenged(&response) {
        return Err(AppError::Challenged(SITE));
    }
    let status = response.status();
    if status.as_u16() == 401 {
        return Err(AppError::BadCredentials { site: SITE });
    }
    if !status.is_success() {
        return Err(AppError::Http {
            site: SITE,
            status: status.as_u16(),
        });
    }
    let envelope: Value = response.json().await?;
    response_body(envelope)
}

fn response_body(mut value: Value) -> Result<Value, AppError> {
    if value
        .get("error")
        .is_some_and(|error| !error.is_null() && error != false)
    {
        return Err(AppError::Upstream {
            site: SITE,
            message: tr!(
                "请求未成功，请检查登录状态和投稿访问权限",
                "The request failed. Check your session and post access"
            ),
        });
    }
    value
        .as_object_mut()
        .and_then(|value| value.remove("body"))
        .ok_or_else(|| parse_error("missing response body"))
}

#[derive(Debug, PartialEq)]
enum Target {
    Creator(String),
    Post(u64),
}

#[derive(Debug, PartialEq)]
struct Query {
    target: Target,
    ratings: Vec<Rating>,
}

fn valid_creator(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn number(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| *n > 0)
}

fn target_of(token: &str) -> Option<Target> {
    if let Some(creator) = token.strip_prefix("creator:") {
        return valid_creator(creator).then(|| Target::Creator(creator.to_string()));
    }
    if let Some(id) = token
        .strip_prefix("post:")
        .or_else(|| token.strip_prefix("id:"))
    {
        return id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .map(Target::Post);
    }
    let url = Url::parse(token).ok()?;
    if url.scheme() != "https"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let host = url.host_str()?;
    let segments: Vec<&str> = url
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect();
    let (creator, rest) = if host == "fanbox.cc" || host == "www.fanbox.cc" {
        (segments.first()?.strip_prefix('@')?, &segments[1..])
    } else {
        let creator = host.strip_suffix(".fanbox.cc")?;
        if ["api", "downloads", "www", "payment"].contains(&creator) {
            return None;
        }
        (creator, &segments[..])
    };
    if !valid_creator(creator) {
        return None;
    }
    match rest {
        [] | ["posts"] => Some(Target::Creator(creator.to_string())),
        ["posts", id] => id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .map(Target::Post),
        _ => None,
    }
}

fn parse_query(value: &str) -> Result<Query, AppError> {
    let mut target = None;
    let mut ratings = Vec::new();
    for token in value.split_whitespace() {
        if let Some(list) = token.strip_prefix("rating:") {
            for rating in list.split(',') {
                ratings.push(Rating::parse(rating).ok_or_else(|| {
                    AppError::InvalidInput(tr!(
                        "FANBOX 分级条件无效",
                        "Invalid FANBOX rating filter"
                    ))
                })?);
            }
        } else if let Some(found) = target_of(token) {
            if target.is_some() {
                return Err(AppError::InvalidInput(tr!(
                    "一次只能查询一位 FANBOX 作者或一篇投稿",
                    "Enter one FANBOX creator or post at a time"
                )));
            }
            target = Some(found);
        } else {
            return Err(AppError::InvalidInput(tr!(
                "请输入 creator:作者 ID 或 FANBOX 作者、投稿链接",
                "Enter creator:creator ID or a FANBOX creator or post URL"
            )));
        }
    }
    Ok(Query {
        target: target.ok_or_else(|| {
            AppError::InvalidInput(tr!(
                "请输入 creator:作者 ID 或 FANBOX 作者、投稿链接",
                "Enter creator:creator ID or a FANBOX creator or post URL"
            ))
        })?,
        ratings,
    })
}

fn page_urls(body: Value, creator: &str) -> Result<Vec<Url>, AppError> {
    let array = body
        .get("pageUrls")
        .unwrap_or(&body)
        .as_array()
        .ok_or_else(|| parse_error("missing pageUrls array"))?;
    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    for entry in array {
        let url = entry
            .as_str()
            .and_then(|value| Url::parse(value).ok())
            .ok_or_else(|| parse_error("invalid page URL"))?;
        let params: Vec<_> = url.query_pairs().collect();
        if !trusted_url(&url, API_HOST)
            || url.path() != "/post.listCreator"
            || params.iter().filter(|(key, _)| key == "creatorId").count() != 1
            || !params
                .iter()
                .any(|(key, value)| key == "creatorId" && value == creator)
            || params.iter().filter(|(key, _)| key == "limit").count() != 1
            || !params
                .iter()
                .any(|(key, value)| key == "limit" && value == "10")
        {
            return Err(parse_error("unexpected creator pagination URL"));
        }
        if seen.insert(url.to_string()) {
            urls.push(url);
        }
    }
    Ok(urls)
}

fn listed_ids(body: Value) -> Result<Vec<u64>, AppError> {
    let array = body
        .get("posts")
        .unwrap_or(&body)
        .as_array()
        .ok_or_else(|| parse_error("missing posts array"))?;
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for item in array {
        let id = item
            .get("id")
            .and_then(number)
            .ok_or_else(|| parse_error("missing post ID"))?;
        image_id(id, 0)?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

async fn creator_pages(
    net: &Net,
    credentials: Option<&Credentials>,
    creator: &str,
) -> Result<Vec<Url>, AppError> {
    page_urls(
        get(
            net,
            credentials,
            api_url("post.paginateCreator", &[("creatorId", creator)]),
        )
        .await?,
        creator,
    )
}

async fn post(net: &Net, credentials: Option<&Credentials>, id: u64) -> Result<Value, AppError> {
    image_id(id, 0)?;
    let body = get(
        net,
        credentials,
        api_url("post.info", &[("postId", &id.to_string())]),
    )
    .await?;
    let value = body.get("post").cloned().unwrap_or(body);
    if value.get("id").and_then(number) != Some(id) {
        return Err(parse_error("post ID does not match the requested post"));
    }
    Ok(value)
}

fn global_error(error: &AppError) -> bool {
    matches!(error, AppError::BadCredentials { .. } | AppError::Challenged(_) | AppError::Http { status: 429, .. })
}

fn matches_rating(post: &Post, ratings: &[Rating]) -> bool {
    ratings.is_empty() || post.rating.is_some_and(|rating| ratings.contains(&rating))
}

pub async fn search(
    net: &Net,
    credentials: Option<&Credentials>,
    query: &str,
    page: &Page,
) -> Result<(Vec<Post>, usize), AppError> {
    let query = parse_query(query)?;
    match (&query.target, page) {
        (Target::Post(id), Page::Number(1)) => {
            let mut posts = post_items(&post(net, credentials, *id).await?, true)?;
            posts.retain(|post| matches_rating(post, &query.ratings));
            Ok((posts, 1))
        }
        (Target::Creator(creator), Page::Number(page)) => {
            let urls = creator_pages(net, credentials, creator).await?;
            let index = page.saturating_sub(1) as usize;
            let Some(url) = urls.get(index) else {
                return Ok((Vec::new(), 0));
            };
            let ids = listed_ids(get(net, credentials, url.clone()).await?)?;
            let fetched = if index + 1 < urls.len() {
                PAGE_SIZE as usize
            } else {
                ids.len()
            };
            let posts = load_creator_page(&ids, &query.ratings, |id| post(net, credentials, id)).await?;
            Ok((posts, fetched))
        }
        (Target::Creator(creator), Page::After(after)) => {
            creator_after(net, credentials, creator, *after, &query.ratings).await
        }
        _ => Ok((Vec::new(), 0)),
    }
}

async fn load_creator_page<F, Fut>(ids: &[u64], ratings: &[Rating], mut load: F) -> Result<Vec<Post>, AppError>
where F: FnMut(u64) -> Fut, Fut: std::future::Future<Output = Result<Value, AppError>> {
    let mut posts = Vec::new();
    for &id in ids {
        match load(id).await.and_then(|value| post_items(&value, false)) {
            Ok(items) => posts.extend(items.into_iter().filter(|post| matches_rating(post, ratings))),
            Err(error) if global_error(&error) => return Err(error),
            Err(error) => log::warn!("FANBOX 投稿 {id} 读取失败：{error}"),
        }
    }
    Ok(posts)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriptionState {
    pub high_water: u64,
    pub published_water: Option<i64>,
    #[serde(default)]
    pub boundary_ids: BTreeSet<u64>,
    #[serde(default)]
    pub pending: Vec<u64>,
}

#[derive(Debug, Clone)]
struct ListedPost {
    id: u64,
    published: Option<i64>,
}

fn listed_posts(body: Value) -> Result<Vec<ListedPost>, AppError> {
    let values = body.get("posts").unwrap_or(&body).as_array().ok_or_else(|| parse_error("missing posts array"))?;
    let mut posts = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let id = value.get("id").and_then(number).ok_or_else(|| parse_error("missing post ID"))?;
        image_id(id, 0)?;
        if seen.insert(id) {
            posts.push(ListedPost {
                id,
                published: value.get("publishedDatetime").and_then(Value::as_str).and_then(super::timestamp::parse),
            });
        }
    }
    Ok(posts)
}

impl SubscriptionState {
    pub fn legacy(after: u64, created_at: i64) -> Self {
        // 旧订阅没有记录受限投稿，首次升级检查回看订阅建立之后的索引。
        Self { high_water: split_id(after).0, published_water: Some(created_at), ..Self::default() }
    }

    pub fn watermark(&self) -> u64 { self.high_water.saturating_mul(PAGE_FACTOR).saturating_add(COVER_INDEX as u64) }

    fn newer(&self, post: &ListedPost) -> bool {
        match (post.published, self.published_water) {
            (Some(date), Some(water)) => date > water || (date == water && !self.boundary_ids.contains(&post.id)),
            _ => post.id > self.high_water,
        }
    }

    fn observe(&mut self, posts: &[ListedPost]) {
        for post in posts {
            self.high_water = self.high_water.max(post.id);
            if let Some(date) = post.published {
                if self.published_water.is_none_or(|water| date > water) {
                    self.published_water = Some(date);
                    self.boundary_ids.clear();
                }
                if self.published_water == Some(date) { self.boundary_ids.insert(post.id); }
            }
        }
    }
}

pub async fn subscription_baseline(net: &Net, credentials: Option<&Credentials>, query: &str) -> Result<SubscriptionState, AppError> {
    let query = parse_query(query)?;
    let Target::Creator(creator) = &query.target else {
        return Err(AppError::InvalidInput(tr!("请订阅 FANBOX 作者", "Subscribe to a FANBOX creator")));
    };
    let urls = creator_pages(net, credentials, creator).await?;
    let mut state = SubscriptionState::default();
    if let Some(url) = urls.first() {
        state.observe(&listed_posts(get(net, credentials, url.clone()).await?)?);
    }
    Ok(state)
}

async fn discover<F, Fut>(state: &mut SubscriptionState, urls: Vec<Url>, mut load: F) -> Result<Vec<u64>, AppError>
where F: FnMut(Url) -> Fut, Fut: std::future::Future<Output = Result<Value, AppError>> {
    let previous = state.clone();
    let mut discovered = BTreeSet::new();
    for url in urls {
        let posts = listed_posts(load(url).await?)?;
        let newer: Vec<_> = posts.iter().filter(|post| previous.newer(post)).map(|post| post.id).collect();
        state.observe(&posts);
        if newer.is_empty() { break; }
        discovered.extend(newer);
    }
    Ok(discovered.into_iter().collect())
}

pub async fn subscription_discover(net: &Net, credentials: Option<&Credentials>, query: &str, state: &mut SubscriptionState) -> Result<Vec<u64>, AppError> {
    let query = parse_query(query)?;
    let Target::Creator(creator) = query.target else { return Err(parse_error("subscription requires a creator")); };
    let urls = creator_pages(net, credentials, &creator).await?;
    let fresh = discover(state, urls, |url| get(net, credentials, url)).await?;
    let mut seen: HashSet<u64> = state.pending.iter().copied().collect();
    // 每次检查都重试一轮未能读取的投稿，新投稿不受失败项阻塞。
    let mut work = state.pending.clone();
    for id in fresh {
        if seen.insert(id) { state.pending.push(id); work.push(id); }
    }
    Ok(work)
}

async fn load_subscription_batch<F, Fut>(state: &mut SubscriptionState, ids: &[u64], ratings: &[Rating], mut load: F) -> Result<Vec<Post>, AppError>
where F: FnMut(u64) -> Fut, Fut: std::future::Future<Output = Result<Value, AppError>> {
    let mut posts = Vec::new();
    for &id in ids {
        match load(id).await.and_then(|value| {
            if value.get("body").is_none_or(Value::is_null) || value.get("isRestricted").and_then(Value::as_bool) == Some(true) {
                return Err(restricted());
            }
            post_items(&value, false)
        }) {
            Ok(items) => {
                posts.extend(items.into_iter().filter(|post| matches_rating(post, ratings)));
                state.pending.retain(|pending| *pending != id);
            }
            Err(error) if global_error(&error) => return Err(error),
            Err(error) => log::warn!("FANBOX 投稿 {id} 留待下次检查：{error}"),
        }
    }
    Ok(posts)
}

pub async fn subscription_batch(net: &Net, credentials: Option<&Credentials>, query: &str, state: &mut SubscriptionState, ids: &[u64]) -> Result<Vec<Post>, AppError> {
    let query = parse_query(query)?;
    load_subscription_batch(state, ids, &query.ratings, |id| post(net, credentials, id)).await
}

async fn creator_after(net: &Net, credentials: Option<&Credentials>, creator: &str, after: u64, ratings: &[Rating]) -> Result<(Vec<Post>, usize), AppError> {
    let query = format!("creator:{creator}");
    let mut state = SubscriptionState { high_water: split_id(after).0, ..SubscriptionState::default() };
    let work = subscription_discover(net, credentials, &query, &mut state).await?;
    let posts = load_subscription_batch(&mut state, &work, ratings, |id| post(net, credentials, id)).await?;
    Ok((posts, work.len()))
}

#[derive(Clone, Copy)]
enum Media<'a> {
    Image(&'a Value),
    File(&'a Value),
}

fn post_items(value: &Value, single_post: bool) -> Result<Vec<Post>, AppError> {
    let id = value
        .get("id")
        .and_then(number)
        .ok_or_else(|| parse_error("missing post ID"))?;
    image_id(id, 0)?;
    let body = value
        .get("body")
        .ok_or_else(|| parse_error("missing post content"))?;
    if body.is_null() || value.get("isRestricted").and_then(Value::as_bool) == Some(true) {
        return if single_post {
            Err(restricted())
        } else {
            Ok(Vec::new())
        };
    }
    let cover = match value.get("coverImageUrl") {
        None | Some(Value::Null) => None,
        Some(Value::String(raw)) if raw.trim().is_empty() => None,
        Some(Value::String(raw)) => Some(cover_url(raw, id).ok_or_else(|| parse_error("invalid cover URL"))?),
        _ => return Err(parse_error("invalid cover URL")),
    };
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| parse_error("missing post type"))?;
    let list: Vec<Media<'_>> = match kind {
        "image" => body
            .get("images")
            .and_then(Value::as_array)
            .ok_or_else(|| parse_error("missing images array"))?
            .iter()
            .map(Media::Image)
            .collect(),
        "file" => body
            .get("files")
            .and_then(Value::as_array)
            .ok_or_else(|| parse_error("missing files array"))?
            .iter()
            .map(Media::File)
            .collect(),
        "article" => {
            let blocks = body
                .get("blocks")
                .and_then(Value::as_array)
                .ok_or_else(|| parse_error("missing article blocks"))?;
            let mut found = Vec::new();
            for block in blocks {
                let mapping = match block.get("type").and_then(Value::as_str) {
                    Some("image") => Some(("imageId", "imageMap", true)),
                    Some("file") => Some(("fileId", "fileMap", false)),
                    _ => None,
                };
                if let Some((id_field, map_field, is_image)) = mapping {
                    let key = block
                        .get(id_field)
                        .and_then(Value::as_str)
                        .ok_or_else(|| parse_error("missing article resource ID"))?;
                    let item = body
                        .get(map_field)
                        .and_then(|map| map.get(key))
                        .ok_or_else(|| parse_error("missing article resource"))?;
                    found.push(if is_image {
                        Media::Image(item)
                    } else {
                        Media::File(item)
                    });
                }
            }
            found
        }
        "text" | "video" | "entry" => Vec::new(),
        _ => return Err(parse_error("unsupported post type")),
    };
    if list.len() + usize::from(cover.is_some()) > PAGE_FACTOR as usize {
        return Err(parse_error("post contains more than 1000 resources"));
    }
    if list.is_empty() && cover.is_none() {
        return Ok(Vec::new());
    }
    let creator = value
        .get("creatorId")
        .and_then(Value::as_str)
        .filter(|value| valid_creator(value))
        .ok_or_else(|| parse_error("invalid creator ID"))?;
    let artist = value
        .pointer("/user/name")
        .and_then(Value::as_str)
        .unwrap_or(creator)
        .to_string();
    let tags: Vec<String> = value
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let rating = if value
        .get("hasAdultContent")
        .and_then(Value::as_bool)
        .ok_or_else(|| parse_error("missing content rating"))?
    {
        Rating::Explicit
    } else {
        Rating::General
    };
    let mut posts = Vec::new();
    if let Some(original) = cover {
        let extension = original.path().rsplit_once('.').expect("validated cover extension").1.to_ascii_lowercase();
        posts.push(Post {
            source: Source::Fanbox,
            id: image_id(id, COVER_INDEX)?,
            md5: None,
            width: 1,
            height: 1,
            rating: Some(rating),
            score: value.get("likeCount").and_then(Value::as_i64).unwrap_or(0),
            fav_count: None,
            file_ext: extension,
            file_size: None,
            file_name: None,
            download_index: Some(0),
            title: value.get("title").and_then(Value::as_str).map(str::to_string),
            file_url: Some(original.to_string()),
            sample_url: Some(original.to_string()),
            thumb_url: Some(original.to_string()),
            created_at: value.get("publishedDatetime").and_then(Value::as_str).map(str::to_string),
            post_url: format!("{REFERER_URL}@{creator}/posts/{id}"),
            tags: PostTags { artist: vec![artist.clone()], general: tags.clone(), ..PostTags::default() },
            pages: None,
        });
    }
    let mut image_index = 0;
    let mut file_index = list.iter().filter(|media| matches!(media, Media::Image(_))).count();
    for media in list {
        let (item, is_image, name_index) = match media {
            Media::Image(item) => {
                image_index += 1;
                (item, true, image_index)
            }
            Media::File(item) => {
                file_index += 1;
                (item, false, file_index)
            }
        };
        let original = item
            .get(if is_image { "originalUrl" } else { "url" })
            .and_then(Value::as_str)
            .and_then(|value| Url::parse(value).ok())
            .filter(|url| trusted_url(url, MEDIA_HOST))
            .ok_or_else(|| parse_error("invalid resource URL"))?;
        let extension = item
            .get("extension")
            .and_then(Value::as_str)
            .ok_or_else(|| parse_error("missing resource extension"))?
            .to_ascii_lowercase();
        if extension.is_empty()
            || extension.len() > 16
            || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(parse_error("invalid resource extension"));
        }
        if is_image
            && !matches!(
                extension.as_str(),
                "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "avif"
            )
        {
            continue;
        }
        let previewable = matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "avif");
        let thumbnail = previewable.then(|| {
            item.get("thumbnailUrl")
                .and_then(Value::as_str)
                .and_then(|value| Url::parse(value).ok())
                .filter(|url| trusted_url(url, MEDIA_HOST))
                .unwrap_or_else(|| original.clone())
                .to_string()
        });
        let dimension = |name: &str| -> Result<u32, AppError> {
            if !is_image {
                return Ok(1);
            }
            item.get(name)
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| parse_error(format!("invalid image {name}")))
        };
        let file_name = if is_image {
            None
        } else {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| parse_error("missing attachment name"))?;
            let suffix = format!(".{extension}");
            Some(if name.to_ascii_lowercase().ends_with(&suffix) {
                name.to_string()
            } else {
                format!("{name}{suffix}")
            })
        };
        let resource = resource_id(id, if is_image { "image" } else { "file" }, item.get("id").and_then(Value::as_str).ok_or_else(|| parse_error("missing resource ID"))?)?;
        if posts.iter().any(|post| post.id == resource) { continue; }
        posts.push(Post {
            source: Source::Fanbox,
            id: resource,
            md5: None,
            width: dimension("width")?,
            height: dimension("height")?,
            rating: Some(rating),
            score: value.get("likeCount").and_then(Value::as_i64).unwrap_or(0),
            fav_count: None,
            file_ext: extension,
            file_size: (!is_image)
                .then(|| {
                    item.get("size")
                        .and_then(Value::as_u64)
                        .filter(|size| *size > 0)
                })
                .flatten(),
            file_name,
            download_index: Some(name_index as u32),
            title: value
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string),
            file_url: Some(original.to_string()),
            sample_url: thumbnail.clone(),
            thumb_url: thumbnail,
            created_at: value
                .get("publishedDatetime")
                .and_then(Value::as_str)
                .map(str::to_string),
            post_url: format!("{REFERER_URL}@{creator}/posts/{id}"),
            tags: PostTags {
                artist: vec![artist.clone()],
                general: tags.clone(),
                ..PostTags::default()
            },
            pages: None,
        });
    }
    Ok(posts)
}

pub async fn count(
    _net: &Net,
    _credentials: Option<&Credentials>,
    query: &str,
) -> Result<Option<u64>, AppError> {
    parse_query(query)?;
    Ok(None)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatorSupport {
    pub plan_title: Option<String>,
    pub fee: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteCreator {
    pub id: String,
    pub name: String,
    pub service: String,
    pub updated: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub support: Option<CreatorSupport>,
    #[serde(default)]
    pub support_status: Option<bool>,
}

pub async fn favorite_creators(
    net: &Net,
    credentials: Option<&Credentials>,
    mode: &str,
) -> Result<Vec<FavoriteCreator>, AppError> {
    let credentials = credentials.ok_or(AppError::FavoritesSignIn(SITE))?;
    let (endpoint, field) = match mode {
        "following" => ("creator.listFollowing", "creators"),
        "supporting" => ("plan.listSupporting", "plans"),
        _ => {
            return Err(AppError::InvalidInput(tr!(
                "FANBOX 作者列表类型无效",
                "Invalid FANBOX creator list type"
            )))
        }
    };
    let mut result = creators(
        get(net, Some(credentials), api_url(endpoint, &[])).await?,
        field,
    )?;
    if mode == "following" {
        match get(net, Some(credentials), api_url("plan.listSupporting", &[]))
            .await
            .and_then(|body| creators(body, "plans"))
        {
            Ok(plans) => merge_support(&mut result, Some(&plans)),
            Err(error) => {
                log::warn!("FANBOX 赞助状态读取失败：{error}");
                merge_support(&mut result, None);
            }
        }
    }
    Ok(result)
}

fn merge_support(followed: &mut [FavoriteCreator], plans: Option<&[FavoriteCreator]>) {
    let by_id = plans.map(|plans| {
        let mut by_id = HashMap::new();
        for plan in plans {
            by_id.entry(plan.id.as_str()).or_insert(plan);
        }
        by_id
    });
    for creator in followed {
        let plan = by_id.as_ref().and_then(|plans| plans.get(creator.id.as_str()));
        creator.support_status = by_id.as_ref().map(|_| plan.is_some());
        creator.support = plan.and_then(|plan| plan.support.clone());
    }
}

fn creators(body: Value, field: &str) -> Result<Vec<FavoriteCreator>, AppError> {
    let entries = body
        .get(field)
        .unwrap_or(&body)
        .as_array()
        .ok_or_else(|| parse_error("missing creator list"))?;
    let mut seen = HashSet::new();
    let mut creators = Vec::new();
    for entry in entries {
        let id = entry
            .get("creatorId")
            .and_then(Value::as_str)
            .filter(|id| valid_creator(id))
            .ok_or_else(|| parse_error("invalid creator ID"))?;
        if seen.insert(id) {
            creators.push(FavoriteCreator {
                id: id.into(),
                name: entry
                    .pointer("/user/name")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .into(),
                service: "fanbox".into(),
                updated: None,
                avatar_url: entry
                    .pointer("/user/iconUrl")
                    .and_then(Value::as_str)
                    .and_then(|value| Url::parse(value.trim()).ok())
                    .filter(|url| trusted_url(url, "pixiv.pximg.net") || trusted_url(url, MEDIA_HOST))
                    .map(|url| url.to_string()),
                support: (field == "plans").then(|| CreatorSupport {
                    plan_title: entry
                        .get("title")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|title| !title.is_empty())
                        .map(str::to_string),
                    fee: entry.get("fee").and_then(Value::as_u64).filter(|fee| *fee <= MAX_SAFE_INTEGER),
                }),
                support_status: (field == "plans").then_some(true),
            });
        }
    }
    Ok(creators)
}

/// 首页 metadata 里的当前用户 ID 只有登录后才存在，关注或支援列表为空不能用来判断登录。
pub async fn verify(net: &Net, credentials: &Credentials) -> Result<String, AppError> {
    let response = net
        .fanbox
        .send(
            request(
                net,
                Url::parse(REFERER_URL).expect("fixed FANBOX homepage"),
                Some(credentials),
            )?
            .header(ACCEPT, "text/html"),
        )
        .await?;
    if net::challenged(&response) {
        return Err(AppError::Challenged(SITE));
    }
    let status = response.status();
    if status.is_redirection() || status.as_u16() == 401 {
        return Err(AppError::BadCredentials { site: SITE });
    }
    if !status.is_success() {
        return Err(AppError::Http {
            site: SITE,
            status: status.as_u16(),
        });
    }
    account_from_html(&response.text().await?)
}

fn account_from_html(html: &str) -> Result<String, AppError> {
    for part in html.split('<').skip(1) {
        if !part
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("meta"))
            || !part.as_bytes().get(4).is_some_and(u8::is_ascii_whitespace)
        {
            continue;
        }
        let attributes = html_attributes(&part[4..]);
        if !attributes
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("name") && value == "metadata")
        {
            continue;
        }
        let content = attributes
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content"))
            .map(|(_, value)| value)
            .ok_or_else(|| parse_error("missing homepage metadata content"))?;
        let metadata: Value = serde_json::from_str(&decode_entities(content))
            .map_err(|_| parse_error("invalid homepage metadata"))?;
        let user = metadata
            .pointer("/context/user")
            .ok_or(AppError::BadCredentials { site: SITE })?;
        let id = user
            .get("userId")
            .and_then(number)
            .ok_or(AppError::BadCredentials { site: SITE })?;
        return Ok(user
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string()));
    }
    Err(parse_error("missing homepage metadata"))
}

fn html_attributes(mut input: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    loop {
        input = input.trim_start();
        if input.is_empty() || input.starts_with(['>', '/']) {
            break;
        }
        let end = input
            .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '>' | '/'))
            .unwrap_or(input.len());
        if end == 0 {
            break;
        }
        let name = &input[..end];
        input = input[end..].trim_start();
        if !input.starts_with('=') {
            continue;
        }
        input = input[1..].trim_start();
        let value;
        if input.starts_with(['\'', '"']) {
            let quote = input.as_bytes()[0] as char;
            input = &input[1..];
            let Some(end) = input.find(quote) else {
                break;
            };
            value = input[..end].to_string();
            input = &input[end + 1..];
        } else {
            let end = input
                .find(|c: char| c.is_ascii_whitespace() || c == '>')
                .unwrap_or(input.len());
            value = input[..end].to_string();
            input = &input[end..];
        }
        result.push((name.to_string(), value));
    }
    result
}

fn decode_entities(input: &str) -> String {
    let mut result = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('&') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|n| u32::from_str_radix(n, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(character) = decoded {
            result.push(character);
        } else {
            result.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    result.push_str(rest);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn image(key: &str) -> Value {
        json!({"id": key, "extension": "png", "width": 1200, "height": 1600,
            "originalUrl": format!("https://downloads.fanbox.cc/images/post/42/{key}.png"),
            "thumbnailUrl": format!("https://downloads.fanbox.cc/images/post/42/w/1200/{key}.jpeg")})
    }

    fn raw() -> Value {
        json!({"id":"42", "title":"October sketches", "creatorId":"artist", "user":{"name":"Artist"}, "type":"image",
            "hasAdultContent":false, "isRestricted":false, "tags":["drawing"], "body":{"images":[image("a"),image("b")]}})
    }

    fn cover(post_id: u64) -> String {
        format!("https://pixiv.pximg.net/c/1200x630_90_a2_g5/fanbox/public/images/post/{post_id}/cover/cover-image.jpeg")
    }

    fn attachment(key: &str, name: &str, extension: &str) -> Value {
        json!({"id":key, "name":name, "extension":extension, "size": 12345,
            "url":format!("https://downloads.fanbox.cc/files/post/42/{key}.{extension}")})
    }

    #[test]
    fn credentials_reject_injection_and_debug_redacts() {
        let credentials = Credentials::from_session(" FANBOXSESSID=1234_secret ").unwrap();
        assert_eq!(credentials.session, "1234_secret");
        assert!(!format!("{credentials:?}").contains("secret"));
        for invalid in [
            "",
            "FANBOXSESSID=",
            "abc; other=def",
            "abc\r\nHeader: value",
            "abc\tdef",
            "\"abc\"",
        ] {
            assert!(Credentials::from_session(invalid).is_none(), "{invalid:?}");
        }
    }

    #[test]
    fn queries_only_accept_real_creator_and_post_addresses() {
        assert_eq!(
            parse_query("https://www.fanbox.cc/@artist/posts/42 rating:general").unwrap(),
            Query {
                target: Target::Post(42),
                ratings: vec![Rating::General]
            }
        );
        assert_eq!(
            parse_query("https://artist.fanbox.cc/").unwrap().target,
            Target::Creator("artist".into())
        );
        assert_eq!(
            parse_query("creator:artist_1").unwrap().target,
            Target::Creator("artist_1".into())
        );
        for invalid in [
            "https://www.fanbox.cc.evil/@artist",
            "https://api.fanbox.cc/",
            "https://a.b.fanbox.cc/",
            "https://u:p@artist.fanbox.cc/",
            "http://artist.fanbox.cc/",
            "creator:../x",
            "creator:a creator:b",
            "creator:a rating:unknown",
        ] {
            assert!(parse_query(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn wrappers_and_pagination_boundaries_are_checked() {
        let url = "https://api.fanbox.cc/post.listCreator?creatorId=artist&limit=10";
        assert_eq!(
            page_urls(json!({"pageUrls":[url]}), "artist")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(page_urls(json!([url]), "artist").unwrap().len(), 1);
        for bad in [
            "https://evil.test/post.listCreator?creatorId=artist&limit=10",
            "https://api.fanbox.cc/post.info?creatorId=artist&limit=10",
            "https://api.fanbox.cc/post.listCreator?creatorId=other&limit=10",
            "https://api.fanbox.cc/post.listCreator?creatorId=artist&limit=300",
        ] {
            assert!(page_urls(json!([bad]), "artist").is_err());
        }
        assert_eq!(
            listed_ids(json!({"posts":[{"id":"42"},{"id":"42"},{"id":43}]})).unwrap(),
            vec![42, 43]
        );
        assert!(listed_ids(json!({"items":[]})).is_err());
        assert!(response_body(json!({})).is_err());
        assert!(response_body(json!({"error":true,"body":[]})).is_err());
    }

    #[test]
    fn media_auth_is_scoped_to_exact_https_download_host() {
        let net = Net::new(&crate::settings::ProxySettings::default()).unwrap();
        let credentials = Credentials::from_session("1234_secret").unwrap();
        let download = media_request(
            &net,
            Url::parse("https://downloads.fanbox.cc/image.png").unwrap(),
            Some(&credentials),
        )
        .unwrap()
        .build()
        .unwrap();
        assert!(download.headers()[COOKIE].is_sensitive());
        let public = media_request(
            &net,
            Url::parse("https://pixiv.pximg.net/cover.png").unwrap(),
            Some(&credentials),
        )
        .unwrap()
        .build()
        .unwrap();
        assert!(public.headers().get(COOKIE).is_none());
        for bad in [
            "http://downloads.fanbox.cc/a.png",
            "https://downloads.fanbox.cc.evil/a.png",
            "https://u:p@downloads.fanbox.cc/a.png",
            "https://downloads.fanbox.cc:8443/a.png",
            "https://api.fanbox.cc/a.png",
        ] {
            assert!(media_request(&net, Url::parse(bad).unwrap(), Some(&credentials)).is_err());
        }
    }

    #[test]
    fn images_keep_order_identity_rating_and_access() {
        let mut value = raw();
        let result = post_items(&value, true).unwrap();
        assert_eq!(
            result.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![resource_id(42, "image", "a").unwrap(), resource_id(42, "image", "b").unwrap()]
        );
        assert!(result
            .iter()
            .all(|p| p.pages.is_none() && p.rating == Some(Rating::General)));
        value["hasAdultContent"] = json!(true);
        assert_eq!(
            post_items(&value, true).unwrap()[0].rating,
            Some(Rating::Explicit)
        );
        value["type"] = json!("article");
        value["body"] = json!({"blocks":[{"type":"image","imageId":"b"},{"type":"p","text":"text"},{"type":"image","imageId":"a"}],"imageMap":{"a":image("a"),"b":image("b")}});
        let result = post_items(&value, true).unwrap();
        assert!(result[0].file_url.as_deref().unwrap().ends_with("b.png"));
        assert!(result[1].file_url.as_deref().unwrap().ends_with("a.png"));
        value["body"] = Value::Null;
        value["coverImageUrl"] = json!("https://pixiv.pximg.net/cover.png");
        assert!(post_items(&value, true).is_err());
        assert!(post_items(&value, false).unwrap().is_empty());
    }

    #[test]
    fn image_id_limits_never_collide_or_round_in_javascript() {
        assert_eq!(split_id(image_id(42, 999).unwrap()), (42, 999));
        assert!(image_id(42, 1000).is_err());
        assert!(image_id(u64::MAX, 0).is_err());
        assert!(image_id(MAX_SAFE_INTEGER / 1000, 992).is_err());
        let mut value = raw();
        value["body"]["images"] = Value::Array(vec![image("a"); 1001]);
        assert!(post_items(&value, true).is_err());
    }

    #[test]
    fn cover_is_first_without_changing_body_ids_or_using_cropped_dimensions() {
        let mut value = raw();
        let existing = post_items(&value, true).unwrap();
        value["coverImageUrl"] = json!(cover(42));
        let posts = post_items(&value, true).unwrap();
        assert_eq!(posts.iter().map(|post| post.id).collect::<Vec<_>>(), [42999, existing[0].id, existing[1].id]);
        assert_eq!(posts.iter().map(|post| post.download_index).collect::<Vec<_>>(), [Some(0), Some(1), Some(2)]);
        assert!(is_cover(&posts[0]));
        assert!(!is_cover(&posts[1]));
        assert_eq!(serde_json::to_value(&posts[1..]).unwrap(), serde_json::to_value(existing).unwrap());
        let original = "https://pixiv.pximg.net/fanbox/public/images/post/42/cover/cover-image.jpeg";
        assert_eq!(posts[0].file_url.as_deref(), Some(original));
        assert_eq!(posts[0].thumb_url, posts[0].file_url);
        assert_eq!(posts[0].sample_url, posts[0].file_url);
        assert_eq!((posts[0].width, posts[0].height), (1, 1));
        assert_eq!(posts[0].title.as_deref(), Some("October sketches"));
        assert!(posts[0].file_name.is_none() && posts[0].file_size.is_none());
    }

    #[test]
    fn covers_keep_the_existing_total_resource_limit_and_legacy_slot_identity() {
        let mut value = raw();
        value["body"]["images"] = Value::Array((0..1000).map(|id| image(&id.to_string())).collect());
        let existing = post_items(&value, true).unwrap();
        assert_eq!(existing.last().unwrap().id, resource_id(42, "image", "999").unwrap());
        assert!(!is_cover(existing.last().unwrap()));
        value["coverImageUrl"] = json!(cover(42));
        assert!(post_items(&value, true).is_err());
        value["body"]["images"] = Value::Array((0..999).map(|id| image(&id.to_string())).collect());
        let posts = post_items(&value, true).unwrap();
        assert_eq!(posts.len(), 1000);
        assert_eq!(posts.iter().map(|post| post.id).collect::<HashSet<_>>().len(), 1000);
        assert_eq!(posts.last().unwrap().id, resource_id(42, "image", "998").unwrap());
    }

    #[test]
    fn cover_urls_require_the_exact_host_post_and_known_path() {
        let mut value = raw();
        value["coverImageUrl"] = json!(cover(42));
        let mut parsed = post_items(&value, true).unwrap().remove(0);
        for bad in [
            "https://pixiv.pximg.net.evil.test/fanbox/public/images/post/42/cover/a.jpeg",
            "http://pixiv.pximg.net/fanbox/public/images/post/42/cover/a.jpeg",
            "https://user:password@pixiv.pximg.net/fanbox/public/images/post/42/cover/a.jpeg",
            "https://pixiv.pximg.net:8443/fanbox/public/images/post/42/cover/a.jpeg",
            "https://pixiv.pximg.net/fanbox/public/images/post/43/cover/a.jpeg",
            "https://pixiv.pximg.net/fanbox/public/images/post/42/other/a.jpeg",
            "https://pixiv.pximg.net/c/unknown/fanbox/public/images/post/42/cover/a.jpeg",
            "https://pixiv.pximg.net/fanbox/public/images/post/42/cover/folder/a.jpeg",
            "https://pixiv.pximg.net/fanbox/public/images/post/42/cover/a%2Fother.jpeg",
            "https://pixiv.pximg.net/fanbox/public/images/post/42/cover/a.jpeg#fragment",
            "https://pixiv.pximg.net/fanbox/public/images/post/42/cover/a.html",
        ] {
            value["coverImageUrl"] = json!(bad);
            assert!(post_items(&value, true).is_err(), "{bad}");
            parsed.file_url = Some(bad.into());
            assert!(!is_cover(&parsed), "{bad}");
        }
        parsed.file_url = Some(cover(42));
        assert!(is_cover(&parsed));
        parsed.id = 42000;
        assert!(!is_cover(&parsed));
        parsed.id = 42999;
        parsed.source = Source::Pixiv;
        assert!(!is_cover(&parsed));
    }

    #[test]
    fn accessible_text_video_and_entry_can_have_covers_without_unlocking_restricted_posts() {
        for kind in ["text", "video", "entry"] {
            let mut value = raw();
            value["type"] = json!(kind);
            value["body"] = json!({"text":"Content"});
            assert!(post_items(&value, true).unwrap().is_empty());
            value["coverImageUrl"] = json!(cover(42));
            let posts = post_items(&value, true).unwrap();
            assert_eq!(posts.len(), 1);
            assert!(is_cover(&posts[0]));
            value["isRestricted"] = json!(true);
            assert!(post_items(&value, true).is_err());
            assert!(post_items(&value, false).unwrap().is_empty());
            value["isRestricted"] = json!(false);
            value["body"] = Value::Null;
            assert!(post_items(&value, true).is_err());
            assert!(post_items(&value, false).unwrap().is_empty());
        }
    }

    #[test]
    fn attachment_posts_preserve_names_titles_and_distinct_item_ids() {
        let mut value = raw();
        value["type"] = json!("file");
        value["body"] = json!({"files":[attachment("a","sketches","zip"),attachment("b","sketches.zip","zip"),attachment("c","Layered drawing.PSD","psd")]});
        let posts = post_items(&value, true).unwrap();
        assert_eq!(
            posts.iter().map(|post| post.id).collect::<Vec<_>>(),
            ["a", "b", "c"].map(|key| resource_id(42, "file", key).unwrap()).to_vec()
        );
        assert_eq!(posts[0].file_name.as_deref(), Some("sketches.zip"));
        assert_eq!(posts[1].file_name.as_deref(), Some("sketches.zip"));
        assert_eq!(posts[2].file_name.as_deref(), Some("Layered drawing.PSD"));
        assert_eq!(posts.iter().map(|post| post.download_index).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
        assert!(posts.iter().all(|post| post.width == 1
            && post.height == 1
            && post.sample_url.is_none()
            && post.thumb_url.is_none()
            && post.file_size == Some(12345)
            && post.title.as_deref() == Some("October sketches")));
        value["body"]["files"][0]["size"] = json!(0);
        assert_eq!(post_items(&value, true).unwrap()[0].file_size, None);
        for extension in ["jpg", "jpeg", "png", "gif", "bmp", "webp", "avif"] {
            value["body"]["files"][0] = attachment("preview", "sketch", extension);
            let image_attachment = post_items(&value, true).unwrap().remove(0);
            assert_eq!(image_attachment.file_name, Some(format!("sketch.{extension}")));
            assert_eq!(image_attachment.download_index, Some(1));
            if extension != "bmp" {
                assert_eq!(image_attachment.thumb_url, image_attachment.file_url);
            }
        }
    }

    #[test]
    fn article_download_indices_number_images_before_files_without_changing_resource_ids() {
        let mut value = raw();
        value["type"] = json!("article");
        value["body"] = json!({
            "blocks":[
                {"type":"file","fileId":"preview"},
                {"type":"image","imageId":"b"},
                {"type":"file","fileId":"archive"},
                {"type":"image","imageId":"a"},
                {"type":"file","fileId":"webp"},
                {"type":"file","fileId":"avif"}
            ],
            "imageMap":{"a":image("a"),"b":image("b")},
            "fileMap":{
                "preview":attachment("preview","Original preview","png"),
                "archive":attachment("archive","../原稿:完成.ZIP","zip"),
                "webp":attachment("webp","Original preview","webp"),
                "avif":attachment("avif","Original preview","avif")
            }
        });
        let posts = post_items(&value, true).unwrap();
        assert_eq!(posts.iter().map(|post| post.id).collect::<Vec<_>>(), [("file", "preview"), ("image", "b"), ("file", "archive"), ("image", "a"), ("file", "webp"), ("file", "avif")].map(|(kind, key)| resource_id(42, kind, key).unwrap()));
        assert_eq!(posts.iter().map(|post| post.download_index).collect::<Vec<_>>(), [Some(3), Some(1), Some(4), Some(2), Some(5), Some(6)]);
        assert_eq!(posts[0].file_name.as_deref(), Some("Original preview.png"));
        assert!(posts[1].file_name.is_none());
        assert!(posts[1].file_url.as_deref().unwrap().ends_with("/b.png"));
        assert!(posts[3].file_url.as_deref().unwrap().ends_with("/a.png"));
    }

    #[test]
    fn article_media_keep_mixed_block_order_and_share_one_item_limit() {
        let mut value = raw();
        value["type"] = json!("article");
        value["body"] = json!({
            "blocks":[{"type":"file","fileId":"z"},{"type":"image","imageId":"b"},{"type":"p","text":"body"},{"type":"file","fileId":"p"},{"type":"image","imageId":"a"}],
            "imageMap":{"a":image("a"),"b":image("b")},
            "fileMap":{"z":attachment("z","Archive","zip"),"p":attachment("p","Layers","psd")}
        });
        let posts = post_items(&value, true).unwrap();
        assert_eq!(
            posts
                .iter()
                .map(|post| post.file_ext.as_str())
                .collect::<Vec<_>>(),
            vec!["zip", "png", "psd", "png"]
        );
        assert_eq!(
            posts.iter().map(|post| post.id).collect::<Vec<_>>(),
            [("file", "z"), ("image", "b"), ("file", "p"), ("image", "a")].map(|(kind, key)| resource_id(42, kind, key).unwrap()).to_vec()
        );
        assert!(posts[1].file_name.is_none() && posts[1].thumb_url.is_some());
        assert_eq!(posts[1].title.as_deref(), Some("October sketches"));
        let mut blocks = vec![json!({"type":"image","imageId":"a"}); 500];
        blocks.extend(vec![json!({"type":"file","fileId":"z"}); 501]);
        value["body"]["blocks"] = json!(blocks);
        assert!(post_items(&value, true).is_err());
    }

    #[test]
    fn attachment_permissions_and_url_extension_boundaries_are_enforced() {
        let mut value = raw();
        value["type"] = json!("file");
        value["body"] = json!({"files":[attachment("a","Archive","zip")]});
        value["isRestricted"] = json!(true);
        assert!(post_items(&value, true).is_err());
        assert!(post_items(&value, false).unwrap().is_empty());
        value["isRestricted"] = json!(false);
        for bad in [
            "https://evil.test/file.zip",
            "http://downloads.fanbox.cc/file.zip",
            "https://downloads.fanbox.cc.evil/file.zip",
        ] {
            value["body"]["files"][0]["url"] = json!(bad);
            assert!(post_items(&value, true).is_err());
        }
        value["body"]["files"][0]["url"] = json!("https://downloads.fanbox.cc/files/post/42/a.zip");
        for bad in ["", "../zip", "zip/psd", "zip;bad", "abcdefghijklmnopq"] {
            value["body"]["files"][0]["extension"] = json!(bad);
            assert!(post_items(&value, true).is_err());
        }
    }

    #[test]
    fn login_requires_real_current_user_metadata() {
        let html = "<html><meta content='{&quot;context&quot;:{&quot;user&quot;:{&quot;userId&quot;:&quot;123&quot;,&quot;name&quot;:&quot;A &amp; B&quot;}}}' name='metadata'></html>";
        assert_eq!(account_from_html(html).unwrap(), "A & B");
        for html in ["<meta name='metadata' content='{&quot;context&quot;:{&quot;user&quot;:{&quot;userId&quot;:null}}}'>", "<meta name='metadata' content='{&quot;body&quot;:{&quot;plans&quot;:[]}}'>", "<meta name='unrelated' content='{}'>"] {
            assert!(account_from_html(html).is_err());
        }
        assert_eq!(decode_entities("&#34;x&#x22;&amp;quot;"), "\"x\"&quot;");
    }

    #[test]
    fn creators_deduplicate_plans_without_accepting_malformed_lists() {
        let first_avatar = "https://pixiv.pximg.net/c/160x160/fanbox/public/images/user/1/icon/a.jpeg";
        let values = json!({"plans":[
            {"creatorId":"a","user":{"name":"A","iconUrl":first_avatar}},
            {"creatorId":"a","user":{"name":"A","iconUrl":"https://pixiv.pximg.net/duplicate.jpeg"}},
            {"creatorId":"b","user":{"name":"B"}}
        ]});
        let result = creators(values, "plans").unwrap();
        assert_eq!(
            result
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(result[0].avatar_url.as_deref(), Some(first_avatar));
        assert!(result[1].avatar_url.is_none());
        assert!(creators(json!({}), "plans").is_err());
    }

    #[test]
    fn followed_creators_keep_public_avatars_in_both_response_formats() {
        let avatar = "https://pixiv.pximg.net/c/160x160_90_a2_g5/fanbox/public/images/user/1/icon/a.jpeg";
        let entries = json!([{"creatorId":"artist","user":{"name":"Artist","iconUrl":avatar}}]);
        for body in [json!({"creators":entries.clone()}), entries] {
            let result = creators(body, "creators").unwrap();
            assert_eq!(result[0].avatar_url.as_deref(), Some(avatar));
            assert_eq!(serde_json::to_value(&result[0]).unwrap()["avatarUrl"], avatar);
        }
    }

    #[test]
    fn current_plans_keep_first_duplicate_and_expose_zero_fee_and_title() {
        let result = creators(json!({"plans":[
            {"creatorId":"a","title":"  Starter  ","fee":0,"user":{"name":"A"}},
            {"creatorId":"a","title":"Duplicate","fee":500,"user":{"name":"Other"}},
            {"creatorId":"b","title":"Sketches","fee":1000,"user":{"name":"B"}}
        ]}), "plans").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "A");
        assert_eq!(result[0].support_status, Some(true));
        let support = result[0].support.as_ref().unwrap();
        assert_eq!(support.plan_title.as_deref(), Some("Starter"));
        assert_eq!(support.fee, Some(0));
        let serialized = serde_json::to_value(&result[1]).unwrap();
        assert_eq!(serialized["supportStatus"], true);
        assert_eq!(serialized["support"]["planTitle"], "Sketches");
        assert_eq!(serialized["support"]["fee"], 1000);
    }

    #[test]
    fn following_support_comes_from_current_plans_instead_of_creator_flags() {
        let mut followed = creators(json!({"creators":[
            {"creatorId":"a","isSupported":false,"isStopped":true,"user":{"name":"Followed A"}},
            {"creatorId":"b","isSupported":true,"isStopped":false,"user":{"name":"Followed B"}},
            {"creatorId":"a","user":{"name":"Duplicate A"}}
        ]}), "creators").unwrap();
        assert!(followed.iter().all(|creator| creator.support_status.is_none() && creator.support.is_none()));
        let plans = creators(json!([
            {"creatorId":"a","title":"Art","fee":500,"user":{"name":"Plan A"}},
            {"creatorId":"other","title":"Other","fee":1000}
        ]), "plans").unwrap();
        merge_support(&mut followed, Some(&plans));
        assert_eq!(followed.len(), 2);
        assert_eq!(followed[0].id, "a");
        assert_eq!(followed[0].name, "Followed A");
        assert_eq!(followed[0].support_status, Some(true));
        assert_eq!(followed[0].support.as_ref().unwrap().fee, Some(500));
        assert_eq!(followed[1].support_status, Some(false));
        assert!(followed[1].support.is_none());
    }

    #[test]
    fn empty_plan_results_mean_not_supporting_but_failed_results_remain_unknown() {
        let mut followed = creators(json!([{"creatorId":"a","user":{"name":"A"}}]), "creators").unwrap();
        let plans = creators(json!([{"creatorId":"a","title":"Art","fee":500}]), "plans").unwrap();
        merge_support(&mut followed, Some(&plans));
        assert_eq!(followed[0].support_status, Some(true));
        let empty = creators(json!({"plans":[]}), "plans").unwrap();
        merge_support(&mut followed, Some(&empty));
        assert_eq!(followed[0].support_status, Some(false));
        assert!(followed[0].support.is_none());
        merge_support(&mut followed, Some(&plans));
        merge_support(&mut followed, None);
        assert_eq!(followed[0].support_status, None);
        assert!(followed[0].support.is_none());
        assert_eq!(followed[0].name, "A");
        assert!(creators(json!({"error":true}), "plans").is_err());
    }

    #[test]
    fn optional_plan_fields_reject_invalid_fees_without_changing_support_status() {
        for title in [Value::Null, json!(""), json!("  ")] {
            for fee in [Value::Null, json!(-1), json!(1.5), json!("500"), json!(true), json!(MAX_SAFE_INTEGER + 1)] {
                let result = creators(json!([{"creatorId":"a","title":title,"fee":fee}]), "plans").unwrap();
                assert_eq!(result[0].support_status, Some(true));
                let support = result[0].support.as_ref().unwrap();
                assert!(support.plan_title.is_none());
                assert!(support.fee.is_none());
            }
        }
        let result = creators(json!([{"creatorId":"a"},{"creatorId":"b","fee":MAX_SAFE_INTEGER}]), "plans").unwrap();
        assert!(result[0].support.as_ref().unwrap().plan_title.is_none());
        assert!(result[0].support.as_ref().unwrap().fee.is_none());
        assert_eq!(result[1].support.as_ref().unwrap().fee, Some(MAX_SAFE_INTEGER));
        let old: FavoriteCreator = serde_json::from_value(json!({"id":"a","name":"A","service":"fanbox","updated":null})).unwrap();
        assert!(old.support_status.is_none() && old.support.is_none());
    }

    #[test]
    fn creator_avatars_are_optional_and_only_use_known_https_media_hosts() {
        for field in ["creators", "plans"] {
            for icon in [
                Value::Null,
                json!(""),
                json!("   "),
                json!("http://pixiv.pximg.net/icon.jpeg"),
                json!("https://pixiv.pximg.net.evil.test/icon.jpeg"),
                json!("https://user:secret@pixiv.pximg.net/icon.jpeg"),
                json!("https://pixiv.pximg.net:8443/icon.jpeg"),
                json!("data:image/png;base64,AAAA"),
            ] {
                let entries = json!([{"creatorId":"artist","user":{"name":"Artist","iconUrl":icon}}]);
                assert!(creators(entries, field).unwrap()[0].avatar_url.is_none());
            }
            let entries = json!([{"creatorId":"artist","user":{"name":"Artist"}}]);
            assert!(creators(entries, field).unwrap()[0].avatar_url.is_none());
            let avatar = "https://downloads.fanbox.cc/images/user/1/icon/a.jpeg";
            let entries = json!([{"creatorId":"artist","user":{"name":"Artist","iconUrl":avatar}}]);
            assert_eq!(creators(entries, field).unwrap()[0].avatar_url.as_deref(), Some(avatar));
        }
    }

    #[tokio::test]
    async fn creator_page_skips_a_broken_post_but_reports_expired_login() {
        let posts = load_creator_page(&[41, 42, 43], &[], |id| {
            let mut value = raw(); value["id"] = json!(id);
            std::future::ready(if id == 42 { Err(parse_error("broken image")) } else { Ok(value) })
        }).await.unwrap();
        assert_eq!(posts.iter().map(post_id).collect::<Vec<_>>(), [41, 41, 43, 43]);
        let error = load_creator_page(&[42], &[], |_| std::future::ready(Err(AppError::BadCredentials { site: SITE }))).await.unwrap_err();
        assert!(matches!(error, AppError::BadCredentials { .. }));
        let mut state = SubscriptionState { pending: vec![42], ..Default::default() };
        assert!(load_subscription_batch(&mut state, &[42], &[], |_| std::future::ready(Err(AppError::BadCredentials { site: SITE }))).await.is_err());
        assert_eq!(state.pending, [42]);
    }

    #[test]
    fn resource_identity_survives_insertion_and_reordering() {
        let mut value = raw();
        let original = post_items(&value, true).unwrap();
        value["body"]["images"] = json!([image("new"), image("b"), image("a")]);
        let edited = post_items(&value, true).unwrap();
        assert_eq!(original[0].id, edited[2].id);
        assert_eq!(original[1].id, edited[1].id);
        assert!(!original.iter().any(|post| post.id == edited[0].id));
        assert_eq!(edited[2].download_index, Some(3));
        assert_eq!(post_id(&edited[2]), 42);
        assert!(edited.iter().all(|post| post.id >= (1u64 << 62) && post.id <= i64::MAX as u64));
    }

    #[test]
    fn subscription_baseline_uses_raw_index_even_when_all_posts_are_hidden_or_filtered() {
        let items = listed_posts(json!([{"id":"90","isRestricted":true}, {"id":"89","hasAdultContent":true}])).unwrap();
        let mut state = SubscriptionState::default();
        state.observe(&items);
        assert_eq!(state.watermark(), 90_999);
        assert!(state.pending.is_empty());
        assert!(!state.newer(&ListedPost { id: 89, published: None }));
        assert!(state.newer(&ListedPost { id: 91, published: None }));
    }

    #[tokio::test]
    async fn subscriptions_stop_at_old_indexes_and_find_delayed_publications() {
        let mut state = SubscriptionState { high_water: 100, published_water: Some(1000), boundary_ids: [100].into(), pending: vec![] };
        let pages: Vec<_> = (0..100).map(|n| Url::parse(&format!("https://api.fanbox.cc/page{n}")).unwrap()).collect();
        let mut requests = 0;
        let work = discover(&mut state, pages, |_| {
            requests += 1;
            let posts = match requests {
                1 => json!([{"id":"101"}, {"id":"90","publishedDatetime":"1970-01-01T00:00:02Z"}]),
                _ => json!([{"id":"100","publishedDatetime":"1970-01-01T00:00:01Z"}]),
            };
            std::future::ready(Ok(posts))
        }).await.unwrap();
        assert_eq!(requests, 2);
        assert_eq!(work, [90, 101]);
        assert_eq!(state.high_water, 101);
        assert!(!state.newer(&ListedPost { id: 999, published: Some(1000) }));
        let mut requests = 0;
        let urls = vec![Url::parse("https://api.fanbox.cc/first").unwrap(), Url::parse("https://api.fanbox.cc/second").unwrap()];
        assert!(discover(&mut state, urls, |_| {
            requests += 1;
            std::future::ready(Ok(json!([{"id":"101"},{"id":"90","publishedDatetime":"1970-01-01T00:00:02Z"}])))
        }).await.unwrap().is_empty());
        assert_eq!(requests, 1);
    }

    #[tokio::test]
    async fn subscription_errors_and_locked_posts_are_retried_after_other_posts_advance() {
        let mut state = SubscriptionState { high_water: 44, pending: vec![42, 43, 44], ..SubscriptionState::default() };
        let posts = load_subscription_batch(&mut state, &[42, 43, 44], &[], |id| {
            let mut value = raw(); value["id"] = json!(id);
            if id == 42 { value["body"] = Value::Null; }
            std::future::ready(if id == 43 { Err(parse_error("bad post")) } else { Ok(value) })
        }).await.unwrap();
        assert_eq!(posts.len(), 2);
        assert!(posts.iter().all(|post| post_id(post) == 44));
        assert_eq!(state.pending, [42, 43]);
        // 模拟重启后恢复权限，仍会补上水位之前的两篇投稿。
        let mut reloaded: SubscriptionState = serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        let posts = load_subscription_batch(&mut reloaded, &[42, 43], &[], |id| {
            let mut value = raw(); value["id"] = json!(id); std::future::ready(Ok(value))
        }).await.unwrap();
        assert_eq!(posts.len(), 4);
        assert!(reloaded.pending.is_empty());
        assert_eq!(reloaded.high_water, 44);
    }

    #[tokio::test]
    #[ignore = "requires public FANBOX network access"]
    async fn live_public_post_smoke() {
        let net = Net::new(&crate::settings::ProxySettings::default()).unwrap();
        let (posts, fetched) = search(&net, None, "post:12560223", &Page::Number(1))
            .await
            .unwrap();
        assert_eq!(fetched, 1);
        assert!(!posts.is_empty());
        assert!(is_cover(&posts[0]));
        assert!(posts.iter().any(|post| !is_cover(post)));
        assert!(posts.iter().all(|post| post.source == Source::Fanbox
            && (is_cover(post) || post
                .file_url
                .as_deref()
                .is_some_and(|url| url.starts_with("https://downloads.fanbox.cc/")))));
    }
}
