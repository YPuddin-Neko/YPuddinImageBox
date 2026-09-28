//! Pixiv：用网页版自己的 ajax 接口（PixivBatchDownloader 用的也是这些），登录状态就是 Cookie 里的 PHPSESSID。
//! - 按 tag 搜：`/ajax/search/artworks/{词}`，每页 60 个作品；不登录最多翻 10 页。
//! - 画师的作品：`/ajax/user/{id}/profile/all` 给出全部作品 id，再用 `/ajax/user/{id}/profile/illusts` 每次取 60 个的信息。
//! - 一个作品可以有好几页，下载时才用 `/ajax/illust/{id}/pages` 取每一页的原图地址。
//!
//! 不登录也能搜全年龄作品、下载原图；R-18 作品要登录。原图要带 `Referer: https://www.pixiv.net/`。
//! 帖子 id 是「作品 id × 1000 + 页码」：搜索结果里一个作品一张卡片（第 0 页），图库里每一页各存一条。
//! 查询串里除了用户输入的词，还有程序自己加的 `rating:` 和 `order:date`（见 `build_query` 和 `Sort::term`）。

use reqwest::header::{ACCEPT, COOKIE, REFERER};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use super::{Page, Post, PostTags, Rating, Source};
use crate::error::AppError;
use crate::i18n::{self, tr, Language};
use crate::net::Net;

const BASE: &str = "https://www.pixiv.net";
const SITE: &str = "Pixiv";
pub const REFERER_URL: &str = "https://www.pixiv.net/";
/// 搜索每页的作品数（站点固定）；画师作品每次也取这么多。
pub const PAGE_SIZE: u32 = 60;
/// 一个作品最多 200 页，帖子 id 按 1000 拆成作品 id 和页码。
const PAGE_FACTOR: u64 = 1000;
/// 找新作品时最多往下翻几页。
const MAX_NEW_PAGES: u32 = 5;
/// 动图（ugoira）作品的扩展名，下载时跳过。
const UGOIRA: &str = "ugoira";

#[derive(Clone)]
pub struct Credentials {
    pub user_id: String,
    pub session: String,
}

/// 调试输出里不打印 Cookie。
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials").field("user_id", &self.user_id).field("session", &"***").finish()
    }
}

impl Credentials {
    /// 登录后的 PHPSESSID 形如 `12345678_xxxx`，前面是账号的用户 id；没登录的会话没有这一段。
    pub fn from_session(value: &str) -> Option<Credentials> {
        let session = value.trim().trim_start_matches("PHPSESSID=").trim();
        let (user_id, rest) = session.split_once('_')?;
        (!user_id.is_empty() && user_id.bytes().all(|b| b.is_ascii_digit()) && !rest.is_empty())
            .then(|| Credentials { user_id: user_id.to_string(), session: session.to_string() })
    }
}

/// 动图（ugoira）是一包逐帧的图片，和其他站点的动图压缩包一样暂不下载。
pub fn is_animation(post: &Post) -> bool {
    post.file_ext == UGOIRA
}

pub fn post_id(illust: u64, page: u32) -> u64 {
    illust * PAGE_FACTOR + u64::from(page)
}

/// 帖子 id 拆回（作品 id，页码）。
pub fn split_id(id: u64) -> (u64, u32) {
    (id / PAGE_FACTOR, (id % PAGE_FACTOR) as u32)
}

#[derive(Debug, Default, PartialEq)]
struct Query {
    target: Target,
    /// 搜索的词；`-词` 表示排除，和网页上一样。
    words: Vec<String>,
    /// 为空表示不限。
    ratings: Vec<Rating>,
    oldest: bool,
}

#[derive(Debug, Default, PartialEq)]
enum Target {
    #[default]
    Search,
    User(u64),
    Work(u64),
}

fn parse_query(query: &str) -> Query {
    let mut parsed = Query::default();
    for token in query.split_whitespace() {
        if let Some(id) = user_of(token) {
            parsed.target = Target::User(id);
        } else if let Some(id) = work_of(token) {
            parsed.target = Target::Work(id);
        } else if let Some(list) = token.strip_prefix("rating:") {
            parsed.ratings = list.split(',').filter_map(Rating::parse).collect();
        } else if token == "order:date" {
            parsed.oldest = true;
        } else {
            parsed.words.push(token.to_string());
        }
    }
    parsed
}

/// `user:123`，或者画师主页的地址（`pixiv.net/users/123`、`pixiv.net/en/users/123/illustrations`）。
fn user_of(token: &str) -> Option<u64> {
    match token.strip_prefix("user:") {
        Some(id) => id.parse().ok(),
        None => id_after(token, "users"),
    }
}

/// `id:123`，或者作品地址（`pixiv.net/artworks/123`）。
fn work_of(token: &str) -> Option<u64> {
    match token.strip_prefix("id:") {
        Some(id) => id.parse().ok(),
        None => id_after(token, "artworks"),
    }
}

fn id_after(token: &str, segment: &str) -> Option<u64> {
    let url = Url::parse(token).ok()?;
    let host = url.host_str()?;
    if host != "pixiv.net" && !host.ends_with(".pixiv.net") {
        return None;
    }
    let mut segments = url.path_segments()?;
    segments.by_ref().find(|s| *s == segment)?;
    segments.next()?.parse().ok()
}

/// 全年龄作品按 sl（数值越高越露骨）分成一般、敏感、存疑；R-18、R-18G 算成人。
fn rating(x_restrict: u8, sl: u8) -> Rating {
    match (x_restrict, sl) {
        (1.., _) => Rating::Explicit,
        (_, 6..) => Rating::Questionable,
        (_, 4..) => Rating::Sensitive,
        _ => Rating::General,
    }
}

/// 搜索的 mode：只要成人时是 r18，不要成人时是 safe，其余 all。不登录时只能搜全年龄作品。
fn mode(ratings: &[Rating], signed_in: bool) -> Result<&'static str, AppError> {
    let explicit = ratings.is_empty() || ratings.contains(&Rating::Explicit);
    let others = ratings.is_empty() || ratings.iter().any(|r| *r != Rating::Explicit);
    match (explicit, others) {
        (true, false) if !signed_in => Err(AppError::CredentialsMissing(SITE)),
        (true, false) => Ok("r18"),
        (false, _) => Ok("safe"),
        _ => Ok("all"),
    }
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    error: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    body: Value,
}

/// 作品信息：搜索结果和画师作品列表里的格式一样。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWork {
    id: Option<String>,
    illust_type: Option<u8>,
    x_restrict: Option<u8>,
    sl: Option<u8>,
    url: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    user_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    page_count: Option<u32>,
    create_date: Option<String>,
}

#[derive(Deserialize)]
struct RawPage {
    urls: PageUrls,
    width: u32,
    height: u32,
}

#[derive(Deserialize)]
struct PageUrls {
    original: Option<String>,
    regular: Option<String>,
    small: Option<String>,
}

fn lang() -> &'static str {
    match i18n::current() {
        Language::Zh => "zh",
        Language::En => "en",
    }
}

fn api_url(path: &[&str], params: &[(&str, String)]) -> Url {
    let mut url = Url::parse(BASE).expect("固定的地址");
    url.path_segments_mut().expect("固定的地址").extend(path);
    url.query_pairs_mut().extend_pairs(params).append_pair("lang", lang());
    url
}

async fn get<T: DeserializeOwned>(net: &Net, credentials: Option<&Credentials>, url: Url) -> Result<T, AppError> {
    let mut request = net.client().get(url).header(REFERER, REFERER_URL).header(ACCEPT, "application/json");
    if let Some(credentials) = credentials {
        request = request.header(COOKIE, format!("PHPSESSID={}", credentials.session));
    }
    let response = net.pixiv.send(request).await?;
    let status = response.status();
    let bytes = response.bytes().await?;
    // 出错时站点也会给 JSON 说明（例如作品已删除），有说明就显示说明。
    let envelope = serde_json::from_slice::<Envelope>(&bytes);
    if let Ok(Envelope { error: true, message, .. }) = &envelope {
        return Err(AppError::Upstream { site: SITE, message: message.clone() });
    }
    if !status.is_success() {
        return Err(AppError::Http { site: SITE, status: status.as_u16() });
    }
    let parse = |e: serde_json::Error| AppError::Parse { site: SITE, detail: e.to_string() };
    serde_json::from_value(envelope.map_err(parse)?.body).map_err(parse)
}

/// 取一页作品，返回（作品，站点这一页实际给了几个）。
pub async fn search(
    net: &Net,
    credentials: Option<&Credentials>,
    query: &str,
    page: &Page,
) -> Result<(Vec<Post>, usize), AppError> {
    let query = parse_query(query);
    let (works, fetched) = match (&query.target, page) {
        (Target::Work(id), Page::Number(1)) => (vec![work(net, credentials, *id).await?], 1),
        (Target::Work(_), _) => (Vec::new(), 0),
        (Target::User(user), Page::After(after)) => user_after(net, credentials, *user, split_id(*after).0).await?,
        (Target::User(user), Page::Number(n)) => user_page(net, credentials, *user, &query, *n).await?,
        (Target::Search, Page::After(after)) => search_after(net, credentials, &query, split_id(*after).0).await?,
        (Target::Search, Page::Number(n)) => search_page(net, credentials, &query, *n).await?,
        // Pixiv 按页码翻页，不会用到「id 小于」。
        (_, Page::Before(_)) => (Vec::new(), 0),
    };
    let rated = |post: &Post| query.ratings.is_empty() || post.rating.is_some_and(|r| query.ratings.contains(&r));
    let posts = works.into_iter().filter(rated);
    let posts: Vec<Post> = match &query.target {
        // 画师的作品没有按词筛选的接口，在这里筛。
        Target::User(_) => posts.filter(|post| matches_words(post, &query.words)).collect(),
        _ => posts.collect(),
    };
    Ok((posts, fetched))
}

/// 有要的词全部要有，`-词` 都不能有；tag 按整个词比较，大小写不影响。
fn matches_words(post: &Post, words: &[String]) -> bool {
    let has = |word: &str| post.tags.general.iter().any(|tag| tag.eq_ignore_ascii_case(word));
    words.iter().all(|word| match word.strip_prefix('-') {
        Some(excluded) => !has(excluded),
        None => has(word),
    })
}

async fn search_page(
    net: &Net,
    credentials: Option<&Credentials>,
    query: &Query,
    page: u32,
) -> Result<(Vec<Post>, usize), AppError> {
    let word = query.words.join(" ");
    if word.is_empty() {
        return Err(AppError::InvalidInput(tr!(
            "在 Pixiv 上搜索要输入 tag，或者 user:画师 ID",
            "To search Pixiv, enter tags or user:artist ID"
        )));
    }
    let params = [
        ("word", word.clone()),
        ("order", if query.oldest { "date" } else { "date_d" }.to_string()),
        ("mode", mode(&query.ratings, credentials.is_some())?.to_string()),
        ("p", page.to_string()),
        ("s_mode", "s_tag".to_string()),
        ("type", "all".to_string()),
    ];
    let body: Value = get(net, credentials, api_url(&["ajax", "search", "artworks", &word], &params)).await?;
    let found = &body["illustManga"];
    // 超过最后一页时站点会重复给最后一页，要自己停下。
    if found["lastPage"].as_u64().is_some_and(|last| u64::from(page) > last) {
        return Ok((Vec::new(), 0));
    }
    let data: Vec<Value> = serde_json::from_value(found["data"].clone()).unwrap_or_default();
    let fetched = data.len();
    Ok((data.into_iter().filter_map(work_post).collect(), fetched))
}

/// 比 `after` 新的作品，旧的在前（订阅找新图用）。从最新的一页往下翻，翻到不比它新的为止。
async fn search_after(
    net: &Net,
    credentials: Option<&Credentials>,
    query: &Query,
    after: u64,
) -> Result<(Vec<Post>, usize), AppError> {
    let newest_first =
        Query { oldest: false, words: query.words.clone(), ratings: query.ratings.clone(), ..Query::default() };
    let mut newer = Vec::new();
    let mut fetched = 0;
    for page in 1..=MAX_NEW_PAGES {
        let (posts, count) = search_page(net, credentials, &newest_first, page).await?;
        fetched += count;
        let reached = posts.iter().any(|post| split_id(post.id).0 <= after);
        newer.extend(posts.into_iter().filter(|post| split_id(post.id).0 > after));
        if reached || count == 0 {
            break;
        }
    }
    newer.reverse();
    Ok((newer, fetched))
}

/// 画师的全部作品 id（插画和漫画），新的在前。
async fn user_ids(net: &Net, credentials: Option<&Credentials>, user: u64) -> Result<Vec<u64>, AppError> {
    let body: Value = get(net, credentials, api_url(&["ajax", "user", &user.to_string(), "profile", "all"], &[])).await?;
    let mut ids: Vec<u64> = ["illusts", "manga"]
        .iter()
        .filter_map(|kind| body[kind].as_object())
        .flat_map(|works| works.keys().filter_map(|id| id.parse().ok()))
        .collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    Ok(ids)
}

/// 按 id 取画师作品的信息，顺序和给出的 id 一致；取不到的（例如没登录时的 R-18 作品）跳过。
async fn user_works(net: &Net, credentials: Option<&Credentials>, user: u64, ids: &[u64]) -> Result<Vec<Post>, AppError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut params: Vec<(&str, String)> = ids.iter().map(|id| ("ids[]", id.to_string())).collect();
    params.extend([("work_category", "illustManga".to_string()), ("is_first_page", "0".to_string())]);
    let url = api_url(&["ajax", "user", &user.to_string(), "profile", "illusts"], &params);
    let body: Value = get(net, credentials, url).await?;
    let works = body["works"].as_object().cloned().unwrap_or_default();
    Ok(ids.iter().filter_map(|id| works.get(&id.to_string()).cloned().and_then(work_post)).collect())
}

async fn user_page(
    net: &Net,
    credentials: Option<&Credentials>,
    user: u64,
    query: &Query,
    page: u32,
) -> Result<(Vec<Post>, usize), AppError> {
    let mut ids = user_ids(net, credentials, user).await?;
    if query.oldest {
        ids.reverse();
    }
    let start = (page.max(1) as usize - 1) * PAGE_SIZE as usize;
    let chunk = ids.get(start..).unwrap_or_default();
    let chunk = &chunk[..chunk.len().min(PAGE_SIZE as usize)];
    Ok((user_works(net, credentials, user, chunk).await?, chunk.len()))
}

async fn user_after(
    net: &Net,
    credentials: Option<&Credentials>,
    user: u64,
    after: u64,
) -> Result<(Vec<Post>, usize), AppError> {
    let mut newer: Vec<u64> = user_ids(net, credentials, user).await?.into_iter().filter(|id| *id > after).collect();
    newer.reverse();
    newer.truncate(PAGE_SIZE as usize);
    Ok((user_works(net, credentials, user, &newer).await?, newer.len()))
}

/// 单个作品（输入了作品地址或 id:123）。
async fn work(net: &Net, credentials: Option<&Credentials>, id: u64) -> Result<Post, AppError> {
    let body: Value = get(net, credentials, api_url(&["ajax", "illust", &id.to_string()], &[])).await?;
    // 详情接口的字段名和列表里不同，换成列表的格式再整理。
    let tags: Vec<Value> = body["tags"]["tags"].as_array().cloned().unwrap_or_default();
    let listed = serde_json::json!({
        "id": body["illustId"],
        "illustType": body["illustType"],
        "xRestrict": body["xRestrict"],
        "sl": body["sl"],
        "url": body["urls"]["thumb"],
        "tags": tags.iter().filter_map(|tag| tag["tag"].as_str()).collect::<Vec<_>>(),
        "userName": body["userName"],
        "width": body["width"],
        "height": body["height"],
        "pageCount": body["pageCount"],
        "createDate": body["createDate"],
    });
    work_post(listed).ok_or_else(|| AppError::Parse { site: SITE, detail: format!("作品 {id} 的信息不完整") })
}

fn work_post(value: Value) -> Option<Post> {
    let raw: RawWork = serde_json::from_value(value).ok()?;
    let illust: u64 = raw.id?.parse().ok()?;
    let (width, height) = (raw.width.filter(|w| *w > 0)?, raw.height.filter(|h| *h > 0)?);
    let thumb = raw.url?;
    let post_url = format!("{BASE}/artworks/{illust}");
    Some(Post {
        source: Source::Pixiv,
        id: post_id(illust, 0),
        md5: None,
        width,
        height,
        rating: Some(rating(raw.x_restrict.unwrap_or(0), raw.sl.unwrap_or(0))),
        score: 0,
        fav_count: None,
        // 动图（ugoira）是压缩包，和其他站点的动图一样不下载；其余作品的扩展名要取到原图地址才知道。
        file_ext: if raw.illust_type == Some(2) { UGOIRA.into() } else { String::new() },
        file_size: None,
        // 原图下载时才按页取，这里放作品页，表示这个作品能下载。
        file_url: Some(post_url.clone()),
        sample_url: master_url(&thumb),
        thumb_url: small_url(&thumb).or(Some(thumb)),
        created_at: raw.create_date,
        post_url,
        tags: PostTags { artist: raw.user_name.into_iter().collect(), general: raw.tags, ..PostTags::default() },
        pages: raw.page_count.filter(|pages| *pages > 1),
    })
}

/// 列表里的缩略图是正方形裁切的，换成保持比例、最长边 540 的小图（详情接口里的 small）。
fn small_url(thumb: &str) -> Option<String> {
    let (host, path) = master_path(thumb)?;
    Some(format!("{host}/c/540x540_70{path}"))
}

/// 最长边 1200 的大图（详情接口里的 regular），详情面板用。
fn master_url(thumb: &str) -> Option<String> {
    let (host, path) = master_path(thumb)?;
    Some(format!("{host}{path}"))
}

/// `https://i.pximg.net/c/250x250_80_a2/custom-thumb/img/…/123_p0_custom1200.jpg`
/// → (`https://i.pximg.net`, `/img-master/img/…/123_p0_master1200.jpg`)
fn master_path(thumb: &str) -> Option<(String, String)> {
    let url = Url::parse(thumb).ok()?;
    let path = url.path();
    let start = path.find("/img-master/").or_else(|| path.find("/custom-thumb/"))?;
    let path = path[start..]
        .replacen("/custom-thumb/", "/img-master/", 1)
        .replace("_square1200", "_master1200")
        .replace("_custom1200", "_master1200");
    Some((format!("{}://{}", url.scheme(), url.host_str()?), path))
}

/// 作品的每一页，带原图地址；帖子 id 按页码排。
pub async fn pages(net: &Net, credentials: Option<&Credentials>, work: &Post) -> Result<Vec<Post>, AppError> {
    let (illust, _) = split_id(work.id);
    let raw: Vec<RawPage> = get(net, credentials, api_url(&["ajax", "illust", &illust.to_string(), "pages"], &[])).await?;
    Ok(raw
        .into_iter()
        .enumerate()
        .filter_map(|(index, page)| {
            let original = page.urls.original?;
            let file_ext = original.rsplit_once('.')?.1.to_ascii_lowercase();
            Some(Post {
                id: post_id(illust, index as u32),
                width: page.width,
                height: page.height,
                file_ext,
                file_url: Some(original),
                sample_url: page.urls.regular,
                thumb_url: page.urls.small,
                pages: None,
                ..work.clone()
            })
        })
        .collect())
}

/// 作品总数：搜索给的是站点的统计；画师作品按 id 数算，带了筛选词时算不出来。
pub async fn count(net: &Net, credentials: Option<&Credentials>, query: &str) -> Result<Option<u64>, AppError> {
    let parsed = parse_query(query);
    match parsed.target {
        Target::Work(_) => Ok(Some(1)),
        Target::User(_) if !parsed.words.is_empty() => Ok(None),
        Target::User(user) => Ok(Some(user_ids(net, credentials, user).await?.len() as u64)),
        Target::Search => {
            let word = parsed.words.join(" ");
            let params = [
                ("word", word.clone()),
                ("order", "date_d".to_string()),
                ("mode", mode(&parsed.ratings, credentials.is_some())?.to_string()),
                ("p", "1".to_string()),
                ("s_mode", "s_tag".to_string()),
                ("type", "all".to_string()),
            ];
            let body: Value = get(net, credentials, api_url(&["ajax", "search", "artworks", &word], &params)).await?;
            Ok(body["illustManga"]["total"].as_u64())
        }
    }
}

/// 验证登录：没登录时 `/ajax/user/extra` 会报错；通过后返回账号的名字。
pub async fn verify(net: &Net, credentials: &Credentials) -> Result<String, AppError> {
    let signed_in: Result<Value, AppError> = get(net, Some(credentials), api_url(&["ajax", "user", "extra"], &[])).await;
    if matches!(signed_in, Err(AppError::Upstream { .. })) {
        return Err(AppError::BadCredentials { site: SITE });
    }
    signed_in?;
    let user: Value = get(net, Some(credentials), api_url(&["ajax", "user", &credentials.user_id], &[])).await?;
    Ok(user["name"].as_str().unwrap_or(&credentials.user_id).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(id: &str, x_restrict: u8, sl: u8, pages: u32) -> Value {
        serde_json::json!({
            "id": id, "title": "t", "illustType": 0, "xRestrict": x_restrict, "sl": sl,
            "url": format!("https://i.pximg.net/c/250x250_80_a2/img-master/img/2026/09/28/22/50/15/{id}_p0_square1200.jpg"),
            "tags": ["初音ミク", "VOCALOID"], "userId": "4447171", "userName": "かーやんアート",
            "width": 2894, "height": 4093, "pageCount": pages, "createDate": "2026-09-28T22:50:15+09:00"
        })
    }

    #[test]
    fn parses_queries() {
        assert_eq!(parse_query("初音ミク -R-18 rating:general,sensitive order:date"), Query {
            target: Target::Search,
            words: vec!["初音ミク".into(), "-R-18".into()],
            ratings: vec![Rating::General, Rating::Sensitive],
            oldest: true,
        });
        assert_eq!(parse_query("user:4447171").target, Target::User(4447171));
        assert_eq!(parse_query("https://www.pixiv.net/en/users/4447171/illustrations").target, Target::User(4447171));
        assert_eq!(parse_query("https://www.pixiv.net/artworks/150225552").target, Target::Work(150225552));
        assert_eq!(parse_query("id:150225552").target, Target::Work(150225552));
        // 别的站点的地址当普通的词。
        assert_eq!(parse_query("https://example.com/users/1").target, Target::Search);
    }

    #[test]
    fn turns_listed_work_into_post() {
        let post = work_post(listed("150225552", 0, 2, 3)).unwrap();
        assert_eq!(post.id, 150225552000);
        assert_eq!(split_id(post.id), (150225552, 0));
        assert_eq!(post.rating, Some(Rating::General));
        assert_eq!(post.pages, Some(3));
        assert_eq!(post.tags.artist, vec!["かーやんアート"]);
        assert_eq!(
            post.thumb_url.as_deref(),
            Some("https://i.pximg.net/c/540x540_70/img-master/img/2026/09/28/22/50/15/150225552_p0_master1200.jpg")
        );
        assert_eq!(
            post.sample_url.as_deref(),
            Some("https://i.pximg.net/img-master/img/2026/09/28/22/50/15/150225552_p0_master1200.jpg")
        );
        assert_eq!(post.file_url.as_deref(), Some("https://www.pixiv.net/artworks/150225552"));
        assert_eq!(work_post(listed("1", 1, 6, 1)).unwrap().rating, Some(Rating::Explicit));
        assert_eq!(work_post(listed("1", 0, 6, 1)).unwrap().pages, None);
        // 广告位之类没有 id 的条目跳过。
        assert!(work_post(serde_json::json!({ "isAdContainer": true })).is_none());
    }

    #[test]
    fn custom_thumbnails_map_to_master_images() {
        let thumb = "https://i.pximg.net/c/250x250_80_a2/custom-thumb/img/2026/09/28/22/32/16/150224840_p0_custom1200.jpg";
        assert_eq!(
            small_url(thumb).as_deref(),
            Some("https://i.pximg.net/c/540x540_70/img-master/img/2026/09/28/22/32/16/150224840_p0_master1200.jpg")
        );
    }

    #[test]
    fn reads_login_from_session_cookie() {
        let creds = Credentials::from_session(" PHPSESSID=12345678_AbCdEf ").unwrap();
        assert_eq!((creds.user_id.as_str(), creds.session.as_str()), ("12345678", "12345678_AbCdEf"));
        assert!(Credentials::from_session("abcdef").is_none());
        assert!(Credentials::from_session("12345678_").is_none());
    }

    #[test]
    fn picks_search_mode() {
        use Rating::*;
        assert_eq!(mode(&[], false).unwrap(), "all");
        assert_eq!(mode(&[General, Sensitive], false).unwrap(), "safe");
        assert_eq!(mode(&[Explicit], true).unwrap(), "r18");
        assert!(matches!(mode(&[Explicit], false), Err(AppError::CredentialsMissing(_))));
        assert_eq!(mode(&[General, Explicit], false).unwrap(), "all");
    }

    #[test]
    fn filters_user_works_by_words() {
        let post = work_post(listed("1", 0, 2, 1)).unwrap();
        assert!(matches_words(&post, &["vocaloid".into()]));
        assert!(!matches_words(&post, &["-初音ミク".into()]));
        assert!(matches_words(&post, &[]));
    }

    /// 真实网络、不登录：按 tag 搜、统计、画师作品、作品地址、取每一页的原图、找新作品。
    #[tokio::test]
    #[ignore = "需要网络，手动运行"]
    async fn searches_pixiv_without_login() {
        let net = Net::new(&crate::settings::ProxySettings::default()).unwrap();
        let query = "初音ミク rating:general,sensitive,questionable";
        let (posts, fetched) = search(&net, None, query, &Page::Number(1)).await.unwrap();
        assert_eq!(fetched, 60);
        assert!(posts.len() > 50 && posts.iter().all(|p| p.thumb_url.is_some() && p.created_at.is_some()));
        let total = count(&net, None, query).await.unwrap().unwrap();
        println!("{query}：共 {total} 个作品，第一个 {} {}", posts[0].label(), posts[0].created_at.as_deref().unwrap_or(""));

        // かーやんアート（上面探测时搜到的画师）
        let (works, _) = search(&net, None, "user:4447171", &Page::Number(1)).await.unwrap();
        assert!(!works.is_empty() && works.iter().all(|w| w.tags.artist == ["かーやんアート"]));
        assert!(works.windows(2).all(|w| w[0].id > w[1].id));
        println!("画师作品 {} 个，最新 {}", works.len(), works[0].label());

        let url = format!("https://www.pixiv.net/artworks/{}", split_id(posts[0].id).0);
        let (single, _) = search(&net, None, &url, &Page::Number(1)).await.unwrap();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].id, posts[0].id);

        let multi = posts.iter().find(|p| p.pages.is_some()).unwrap_or(&posts[0]);
        let pages = pages(&net, None, multi).await.unwrap();
        println!("{} 有 {} 页，第一页 {}", multi.label(), pages.len(), pages[0].file_url.as_deref().unwrap_or(""));
        assert_eq!(pages.len() as u32, multi.pages.unwrap_or(1));
        assert!(pages.iter().all(|p| p.file_url.as_deref().is_some_and(|u| u.contains("/img-original/"))));
        assert!(pages.iter().enumerate().all(|(i, p)| split_id(p.id) == (split_id(multi.id).0, i as u32)));

        let after = posts[5].id;
        let (newer, _) = search(&net, None, query, &Page::After(after)).await.unwrap();
        assert!(newer.iter().all(|p| p.id > after) && newer.windows(2).all(|w| w[0].id < w[1].id));
        println!("比 {} 新的 {} 个", posts[5].label(), newer.len());
    }
}
