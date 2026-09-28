//! Yande.re（Moebooru）：`GET /post.json?tags=&page=&limit=`，每页最多 100 条，页码从 1 开始，不用登录。
//! 帖子里的 tag 不带分类，全部归入「一般」。分级只有 s / q / e 三级，s（安全）包括一般和敏感。
//! 上传时间是 Unix 秒；总数只有 `post.xml` 给（根节点的 `count` 属性）。

use reqwest::header::ACCEPT;
use serde::Deserialize;

use super::{non_empty, split_tags, timestamp, Post, PostTags, Rating, Source};
use crate::error::AppError;
use crate::net::Net;

const BASE: &str = "https://yande.re";
const SITE: &str = "Yande.re";

#[derive(Deserialize)]
struct RawPost {
    id: Option<u64>,
    tags: Option<String>,
    created_at: Option<i64>,
    score: Option<i64>,
    md5: Option<String>,
    file_size: Option<u64>,
    file_ext: Option<String>,
    file_url: Option<String>,
    sample_url: Option<String>,
    jpeg_url: Option<String>,
    preview_url: Option<String>,
    rating: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    status: Option<String>,
}

pub async fn search(net: &Net, query: &str, page: u32, limit: u32) -> Result<(Vec<Post>, usize), AppError> {
    parse(&get(net, "post.json", query, page.max(1), limit.min(100)).await?)
}

pub async fn count(net: &Net, query: &str) -> Result<Option<u64>, AppError> {
    let body = get(net, "post.xml", query, 1, 1).await?;
    Ok(count_attribute(&String::from_utf8_lossy(&body)))
}

async fn get(net: &Net, endpoint: &str, query: &str, page: u32, limit: u32) -> Result<Vec<u8>, AppError> {
    let request = net
        .client()
        .get(format!("{BASE}/{endpoint}"))
        .query(&[("tags", query.to_string()), ("page", page.to_string()), ("limit", limit.to_string())])
        .header(ACCEPT, "application/json, application/xml");
    let response = net.api.send(request).await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Http { site: SITE, status: status.as_u16() });
    }
    Ok(response.bytes().await?.to_vec())
}

/// `<posts count="123" offset="0">` 里的 `count`。
fn count_attribute(xml: &str) -> Option<u64> {
    let start = xml.find("<posts")?;
    let end = start + xml[start..].find('>')?;
    let (_, rest) = xml[start..end].split_once("count=\"")?;
    rest.split('"').next()?.parse().ok()
}

pub fn parse(body: &[u8]) -> Result<(Vec<Post>, usize), AppError> {
    let raw: Vec<RawPost> =
        serde_json::from_slice(body).map_err(|e| AppError::Parse { site: SITE, detail: e.to_string() })?;
    let count = raw.len();
    Ok((raw.into_iter().filter_map(normalize).collect(), count))
}

/// 已删除的帖子（按 id 翻页时也会出现）和没有尺寸的早年 Flash 之类都跳过。
fn normalize(raw: RawPost) -> Option<Post> {
    if raw.status.as_deref() == Some("deleted") {
        return None;
    }
    let id = raw.id?;
    let (width, height) = (raw.width.filter(|w| *w > 0)?, raw.height.filter(|h| *h > 0)?);
    let file_url = non_empty(raw.file_url);
    let file_ext = non_empty(raw.file_ext)
        .or_else(|| Some(file_url.as_deref()?.rsplit_once('.')?.1.to_string()))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let sample_url = non_empty(raw.sample_url).or_else(|| non_empty(raw.jpeg_url)).or_else(|| file_url.clone());
    Some(Post {
        source: Source::Yandere,
        id,
        md5: non_empty(raw.md5),
        width,
        height,
        rating: raw.rating.as_deref().and_then(parse_rating),
        score: raw.score.unwrap_or(0),
        fav_count: None,
        file_ext,
        file_size: raw.file_size,
        file_url,
        sample_url,
        thumb_url: non_empty(raw.preview_url),
        created_at: raw.created_at.map(timestamp::iso_utc),
        post_url: format!("{BASE}/post/show/{id}"),
        tags: PostTags { general: split_tags(raw.tags.as_deref()), ..PostTags::default() },
    })
}

/// s（安全）按一般算。
fn parse_rating(value: &str) -> Option<Rating> {
    match value {
        "s" => Some(Rating::General),
        "q" => Some(Rating::Questionable),
        "e" => Some(Rating::Explicit),
        _ => None,
    }
}

/// 分级条件。一般和敏感都对应 s；选了两级时写成排除剩下的那一级（`-rating:e`）。
pub fn rating_term(selected: &[Rating]) -> Option<String> {
    let levels: Vec<char> = [
        ('s', selected.contains(&Rating::General) || selected.contains(&Rating::Sensitive)),
        ('q', selected.contains(&Rating::Questionable)),
        ('e', selected.contains(&Rating::Explicit)),
    ]
    .into_iter()
    .filter_map(|(level, chosen)| chosen.then_some(level))
    .collect();
    match levels.as_slice() {
        [level] => Some(format!("rating:{level}")),
        [_, _] => ['s', 'q', 'e'].into_iter().find(|level| !levels.contains(level)).map(|level| format!("-rating:{level}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &[u8] = br#"[
      {"id":1269665,"tags":"japanese_clothes key  seifuku","created_at":1790597042,"score":3,"md5":"0e00a7e86151037d3049bba62bac6ff4",
       "file_size":666588,"file_ext":"webp","file_url":"https://files.yande.re/image/0e00/yande.re%201269665.webp",
       "sample_url":"https://files.yande.re/sample/0e00/sample.jpg","jpeg_url":"https://files.yande.re/jpeg/0e00/j.jpg",
       "preview_url":"https://assets.yande.re/data/preview/0e/00/0e00.jpg","rating":"s","width":1920,"height":1920},
      {"id":6437,"tags":"flash","created_at":1182690420,"rating":"s","width":null,"height":null},
      {"id":7,"tags":"","created_at":1182690420,"rating":"e","width":10,"height":20,"file_url":"https://files.yande.re/image/a.PNG"},
      {"id":533633,"status":"deleted","rating":"s","width":659,"height":1200,"file_url":null}
    ]"#;

    #[test]
    fn parses_posts_and_skips_unusable_ones() {
        let (posts, count) = parse(LIST).unwrap();
        assert_eq!(count, 4);
        assert_eq!(posts.len(), 2);
        let first = &posts[0];
        assert_eq!((first.id, first.width, first.file_ext.as_str()), (1269665, 1920, "webp"));
        assert_eq!(first.rating, Some(Rating::General));
        assert_eq!(first.tags.general, vec!["japanese_clothes", "key", "seifuku"]);
        assert_eq!(first.created_at.as_deref(), Some("2026-09-28T12:04:02Z"));
        assert_eq!(first.sample_url.as_deref(), Some("https://files.yande.re/sample/0e00/sample.jpg"));
        assert_eq!(first.post_url, "https://yande.re/post/show/1269665");
        // 没写扩展名时从原图地址取。
        assert_eq!((posts[1].file_ext.as_str(), posts[1].rating), ("png", Some(Rating::Explicit)));
    }

    #[test]
    fn reads_count_from_xml() {
        assert_eq!(count_attribute("<?xml version=\"1.0\"?>\n<posts count=\"5213\" offset=\"0\">\n</posts>"), Some(5213));
        assert_eq!(count_attribute("<posts offset=\"0\"></posts>"), None);
    }

    #[test]
    fn maps_ratings_to_three_levels() {
        use Rating::*;
        assert_eq!(rating_term(&[General]).as_deref(), Some("rating:s"));
        assert_eq!(rating_term(&[General, Sensitive]).as_deref(), Some("rating:s"));
        assert_eq!(rating_term(&[Questionable]).as_deref(), Some("rating:q"));
        assert_eq!(rating_term(&[Sensitive, Questionable]).as_deref(), Some("-rating:e"));
        assert_eq!(rating_term(&[General, Explicit]).as_deref(), Some("-rating:q"));
        assert_eq!(rating_term(&[Sensitive, Questionable, Explicit]), None);
        assert_eq!(rating_term(&[]), None);
    }

    /// 真实网络：搜一页、取总数，再按 id 往旧和往新两个方向翻页（找新图的订阅用后者）。
    #[tokio::test]
    #[ignore = "需要网络，手动运行"]
    async fn searches_yandere() {
        use crate::sources::{fetch, Accounts, Page};
        let net = Net::new(&crate::settings::ProxySettings::default()).unwrap();
        let query = "landscape rating:s";
        let (posts, fetched) = search(&net, query, 1, 5).await.unwrap();
        assert_eq!(fetched, 5);
        assert!(posts.iter().all(|p| p.file_url.is_some() && p.md5.is_some()));
        assert!(posts.iter().all(|p| p.created_at.as_deref().and_then(timestamp::parse).is_some()));
        let total = count(&net, query).await.unwrap().unwrap();
        println!("{query}：共 {total} 张，第一张 #{} {}", posts[0].id, posts[0].created_at.as_deref().unwrap_or(""));
        assert!(total > 100);

        let oldest = posts.iter().map(|p| p.id).min().unwrap();
        let accounts = Accounts::default();
        let (older, _) = fetch(&net, &accounts, Source::Yandere, query, &Page::Before(oldest), 5).await.unwrap();
        assert!(!older.is_empty() && older.iter().all(|p| p.id < oldest));
        let (newer, _) = fetch(&net, &accounts, Source::Yandere, query, &Page::After(oldest), 5).await.unwrap();
        let ids: Vec<u64> = newer.iter().map(|p| p.id).collect();
        assert!(!ids.is_empty() && ids.windows(2).all(|w| w[0] < w[1]) && ids[0] > oldest, "{ids:?}");
    }
}
