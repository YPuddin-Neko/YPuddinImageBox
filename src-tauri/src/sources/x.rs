//! X 的浏览器响应解析。
//!
//! X 没有稳定的公开媒体搜索接口。采集窗口在 X 页面里拦截自己的 GraphQL
//! 时间线响应，这里只接收响应正文并提取图片，不保存页面脚本或登录凭据。

use std::collections::HashSet;

use serde_json::{Map, Value};
use url::Url;

use crate::error::AppError;

use super::{timestamp, Post, PostTags, Source};

const SITE: &str = "X";

/// 采集窗口打开的是哪种页面，决定收到的图归哪里：用户的媒体归采集页，自己的喜欢、书签归收藏页。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture {
    /// 这个用户的媒体（用户名不带 @）。
    Media(String),
    Likes,
    Bookmarks,
}

impl Capture {
    pub fn kind(&self) -> &'static str {
        match self {
            Capture::Media(_) => "media",
            Capture::Likes => "likes",
            Capture::Bookmarks => "bookmarks",
        }
    }

    /// 这个页面上收到的响应算不算数。X 改版后查询名换过一次（旧的 `UserMedia`、`Likes`，新的 `likesQuery` 这类），
    /// 所以不看查询名，看采集窗口现在停在哪个页面：媒体要在这个用户的主页下（媒体、帖子、单条帖子都行），
    /// 喜欢在 `/{用户名}/likes`，书签在 `/i/bookmarks`。在首页、搜索页等别处收到的一概不收。
    pub fn accepts(&self, page: &str) -> bool {
        let mut parts = page.trim_matches('/').split('/');
        let (first, second) = (parts.next().unwrap_or_default(), parts.next());
        match self {
            Capture::Media(user) => first.eq_ignore_ascii_case(user),
            Capture::Likes => second == Some("likes") && parts.next().is_none(),
            Capture::Bookmarks => first == "i" && second == Some("bookmarks"),
        }
    }

    /// 媒体只收这个用户自己发的；喜欢、书签里本来就是别人的帖子。
    fn author(&self) -> Option<&str> {
        match self {
            Capture::Media(user) => Some(user),
            _ => None,
        }
    }
}

/// X 的媒体 id 是字符串；图库的旧表使用正整数帖子 id，所以用媒体 key 做稳定的 63 位哈希。
fn media_id(tweet_id: &str, media_id: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{tweet_id}:{media_id}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash & 0x7fff_ffff_ffff_ffff
}

pub fn label_id(id: u64) -> String {
    format!("x-{id:016x}")
}

/// 从响应里取出所有图片。`capture` 是采媒体时，只留这个用户自己发的帖子（首页推荐、引用的帖子里别人的图不要）。
pub fn parse_posts(body: &str, capture: Option<&Capture>) -> Result<Vec<Post>, AppError> {
    let value: Value = serde_json::from_str(body).map_err(|err| AppError::Parse {
        site: SITE,
        detail: err.to_string(),
    })?;
    let mut posts = Vec::new();
    let mut seen = HashSet::new();
    walk(&value, capture.and_then(Capture::author), &mut seen, &mut posts);
    Ok(posts)
}

fn walk(value: &Value, author: Option<&str>, seen: &mut HashSet<u64>, posts: &mut Vec<Post>) {
    match value {
        Value::Array(values) => values.iter().for_each(|value| walk(value, author, seen, posts)),
        Value::Object(object) => {
            parse_tweet(object, author, seen, posts);
            object.values().for_each(|value| walk(value, author, seen, posts));
        }
        _ => {}
    }
}

/// 一条帖子里的图片。认两种格式，作者都在 `core.user_results.result`：
/// - 旧网页：图片在 `legacy.extended_entities.media`（或 `legacy.entities.media`），id、时间、hashtag 也在 `legacy` 里；
/// - 2026 年 9 月起的新网页（Relay）：图片在帖子的 `media_entities2`，id 是 `rest_id`，
///   时间和 hashtag 在 `details`（`created_at_ms`、`hashtag_entities`），`legacy` 里只剩转发、语言这些。
fn parse_tweet(tweet: &Map<String, Value>, author: Option<&str>, seen: &mut HashSet<u64>, posts: &mut Vec<Post>) {
    let legacy = tweet.get("legacy");
    let details = tweet.get("details");
    let Some(media) = tweet
        .get("media_entities2")
        .or_else(|| legacy?.get("extended_entities")?.get("media"))
        .or_else(|| legacy?.get("entities")?.get("media"))
        .and_then(Value::as_array)
    else {
        return;
    };
    let Some(tweet_id) = text(tweet.get("rest_id")).or_else(|| text(legacy?.get("id_str").or_else(|| legacy?.get("id")))) else {
        return;
    };

    let (screen_name, display_name) = user_name(tweet.get("core"));
    if let (Some(author), Some(name)) = (author, screen_name.as_deref()) {
        if !name.eq_ignore_ascii_case(author) {
            return;
        }
    }
    let hashtags = details
        .and_then(|details| details.get("hashtag_entities"))
        .or_else(|| legacy?.get("entities")?.get("hashtags"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tag| text(tag.get("text")))
        .collect::<Vec<_>>();
    let created_at = text(legacy.and_then(|legacy| legacy.get("created_at"))).or_else(|| {
        let millis = details?.get("created_at_ms")?;
        let millis = millis.as_i64().or_else(|| millis.as_str()?.parse().ok())?;
        Some(timestamp::iso_utc(millis.div_euclid(1000)))
    });

    for media in media {
        let Some(media_url) = text(media.get("media_url_https").or_else(|| media.get("media_url"))) else {
            continue;
        };
        if !is_photo(media, &media_url) {
            continue;
        }
        let media_key = text(media.get("id_str").or_else(|| media.get("id")))
            .unwrap_or_else(|| media_url.clone());
        let id = media_id(&tweet_id, &media_key);
        if !seen.insert(id) {
            continue;
        }
        let (width, height) = media_dimensions(media);
        let ext = media_format(media, &media_url);
        let file_url = variant_url(&media_url, &ext, "orig");
        let sample_url = variant_url(&media_url, &ext, "large");
        let thumb_url = variant_url(&media_url, &ext, "small");
        let artist = screen_name
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| display_name.clone());
        let post_url = match artist.as_deref() {
            Some(artist) => format!("https://x.com/{artist}/status/{tweet_id}"),
            None => format!("https://x.com/i/status/{tweet_id}"),
        };
        posts.push(Post {
            source: Source::X,
            id,
            md5: None,
            width,
            height,
            rating: None,
            score: 0,
            fav_count: None,
            file_ext: ext,
            file_name: None,
            title: None,
            file_size: media
                .get("original_info")
                .and_then(|info| info.get("size_bytes"))
                .and_then(Value::as_u64),
            file_url: Some(file_url),
            sample_url: Some(sample_url),
            thumb_url: Some(thumb_url),
            created_at: created_at.clone(),
            post_url,
            tags: PostTags {
                artist: artist.into_iter().collect(),
                general: hashtags.clone(),
                ..PostTags::default()
            },
            pages: None,
        });
    }
}

/// 只要图片。旧网页的 `type` 是 photo / video / animated_gif，新网页可能换了写法：
/// 认不出时按地址判断，图片在 `pbs.twimg.com/media/` 下，视频和动图的封面在别的目录，而且带 `video_info`。
fn is_photo(media: &Value, url: &str) -> bool {
    match media.get("type").and_then(Value::as_str) {
        Some(kind) if kind.eq_ignore_ascii_case("photo") => true,
        Some(kind) if kind.eq_ignore_ascii_case("video") || kind.eq_ignore_ascii_case("animated_gif") => false,
        _ => media.get("video_info").is_none_or(Value::is_null) && Url::parse(url).is_ok_and(|url| url.path().starts_with("/media/")),
    }
}

fn user_name(core: Option<&Value>) -> (Option<String>, Option<String>) {
    let Some(result) = core
        .and_then(|core| core.get("user_results"))
        .and_then(|users| users.get("result"))
    else {
        return (None, None);
    };
    let legacy = result.get("legacy");
    let profile = result.get("core");
    let screen_name = text(profile.and_then(|profile| profile.get("screen_name")))
        .or_else(|| text(legacy.and_then(|legacy| legacy.get("screen_name"))));
    let display_name = text(profile.and_then(|profile| profile.get("name")))
        .or_else(|| text(legacy.and_then(|legacy| legacy.get("name"))));
    (screen_name, display_name)
}

fn media_dimensions(media: &Value) -> (u32, u32) {
    let original = media.get("original_info");
    let sizes = media.get("sizes").and_then(|sizes| sizes.get("large"));
    let width = original
        .or(sizes)
        .and_then(|info| info.get("width"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let height = original
        .or(sizes)
        .and_then(|info| info.get("height"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    (
        width.min(u64::from(u32::MAX)) as u32,
        height.min(u64::from(u32::MAX)) as u32,
    )
}

fn media_format(media: &Value, url: &str) -> String {
    let _ = media;
    Url::parse(url)
        .ok()
        .and_then(|url| url.path().rsplit('.').next().map(str::to_string))
        .filter(|ext| ext.chars().all(|ch| ch.is_ascii_alphabetic()) && ext.len() <= 5)
        .unwrap_or_else(|| "jpg".into())
        .to_ascii_lowercase()
}

fn variant_url(raw: &str, format: &str, name: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return raw.to_string();
    };
    url.query_pairs_mut()
        .append_pair("format", format)
        .append_pair("name", name);
    url.to_string()
}

fn text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_photo_media_from_nested_timeline_response() {
        let body = serde_json::json!({
            "data": {"result": {
                "legacy": {
                    "id_str": "100",
                    "created_at": "Mon Sep 28 10:00:00 +0000 2026",
                    "entities": {"hashtags": [{"text": "art"}]},
                    "extended_entities": {"media": [{
                        "type": "photo",
                        "id_str": "m1",
                        "media_url_https": "https://pbs.twimg.com/media/abc.jpg",
                        "original_info": {"width": 1200, "height": 800}
                    }, {
                        "type": "video",
                        "id_str": "v1",
                        "media_url_https": "https://pbs.twimg.com/media/video.jpg"
                    }]}
                },
                "core": {"user_results": {"result": {"core": {"name": "Artist", "screen_name": "artist"}}}}
            }}
        });
        let posts = parse_posts(&body.to_string(), None).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].source, Source::X);
        assert_eq!(posts[0].tags.artist, ["artist"]);
        assert_eq!(posts[0].tags.general, ["art"]);
        assert!(posts[0].file_url.as_deref().unwrap().contains("name=orig"));
        assert_eq!((posts[0].width, posts[0].height), (1200, 800));
    }

    /// 2026 年 9 月起的新网页：图片在 `media_entities2`，时间、hashtag 在 `details`，`legacy` 里没有图片。
    /// 结构按 X 网页 likesQuery 的 Relay 查询定义整理。
    fn relay_tweet(id: &str, author: &str, media: Value) -> Value {
        serde_json::json!({
            "__typename": "Tweet",
            "rest_id": id,
            "core": {"user_results": {"result": {"__typename": "User", "core": {"name": "Name", "screen_name": author}}}},
            "details": {
                "full_text": "text",
                "created_at_ms": 1790686543000i64,
                "hashtag_entities": [{"indices": [0, 4], "text": "オリジナル"}]
            },
            "legacy": {"lang": "ja", "possibly_sensitive": false},
            "media_entities2": media
        })
    }

    #[test]
    fn parses_relay_timeline_response() {
        let photo = serde_json::json!([
            {"id_str": "m1", "type": "photo", "media_url_https": "https://pbs.twimg.com/media/aaa.jpg", "original_info": {"width": 1400, "height": 2000}},
            {"id_str": "m2", "type": "video", "media_url_https": "https://pbs.twimg.com/ext_tw_video_thumb/1/pu/img/v.jpg", "video_info": {"variants": []}}
        ]);
        // 取值没见过的 type：按地址判断，/media/ 下又没有视频信息的算图片
        let unknown = serde_json::json!([
            {"id_str": "m3", "type": "IMAGE", "media_url_https": "https://pbs.twimg.com/media/bbb.png", "original_info": {"width": 800, "height": 600}},
            {"id_str": "m4", "type": "GIF", "media_url_https": "https://pbs.twimg.com/tweet_video_thumb/g.jpg", "video_info": {}}
        ]);
        let body = serde_json::json!({"data": {"user_result_by_rest_id": {"result": {"timeline": {"timeline": {"instructions": [
            {"type": "TimelineAddEntries", "entries": [
                {"content": {"content": {"tweet_results": {"result": relay_tweet("200", "nimono_", photo)}}}},
                {"content": {"content": {"tweet_results": {"result": relay_tweet("201", "nimono_", unknown)}}}},
                {"content": {"content": {"tweet_results": {"result": relay_tweet("202", "someone_else", serde_json::json!([
                    {"id_str": "m5", "type": "photo", "media_url_https": "https://pbs.twimg.com/media/ccc.jpg"}
                ]))}}}}
            ]}
        ]}}}}}});
        let all = parse_posts(&body.to_string(), None).unwrap();
        assert_eq!(all.len(), 3, "图片 m1、m3、m5，视频和动图不要");
        let first = &all[0];
        assert_eq!(first.tags.artist, ["nimono_"]);
        assert_eq!(first.tags.general, ["オリジナル"]);
        assert_eq!(first.created_at.as_deref(), Some("2026-09-29T12:55:43Z"));
        assert_eq!((first.width, first.height), (1400, 2000));
        assert_eq!(first.post_url, "https://x.com/nimono_/status/200");
        assert!(all[1].file_url.as_deref().unwrap().contains("format=png&name=orig"));
        // 采这个用户的媒体时，别人的帖子不要
        let mine = parse_posts(&body.to_string(), Some(&Capture::Media("Nimono_".into()))).unwrap();
        assert_eq!(mine.len(), 2);
        // 同一张图新旧两种格式算出的 id 一样，已经下载过的不会重复
        assert_eq!(first.id, media_id("200", "m1"));
    }

    #[test]
    fn accepts_responses_on_the_capture_page() {
        let media = Capture::Media("nimono_".into());
        assert!(media.accepts("/nimono_/media"));
        assert!(media.accepts("/Nimono_"));
        assert!(media.accepts("/nimono_/status/1512724979619868672"));
        assert!(!media.accepts("/home"));
        assert!(!media.accepts("/someone/media"));
        assert!(!media.accepts("/i/jf/onboarding/web"));
        assert!(Capture::Likes.accepts("/sora/likes"));
        assert!(!Capture::Likes.accepts("/sora/status/1/likes"));
        assert!(!Capture::Likes.accepts("/home"));
        assert!(Capture::Bookmarks.accepts("/i/bookmarks"));
        assert!(Capture::Bookmarks.accepts("/i/bookmarks/12345"));
        assert!(!Capture::Bookmarks.accepts("/sora/likes"));
        assert_eq!(label_id(12), "x-000000000000000c");
    }
}
