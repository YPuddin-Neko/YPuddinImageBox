//! X 的浏览器响应解析。
//!
//! X 没有稳定的公开媒体搜索接口。采集窗口在 X 页面里拦截自己的 GraphQL
//! 时间线响应，这里只接收响应正文并提取图片，不保存页面脚本或登录凭据。

use std::collections::HashSet;

use serde_json::Value;
use url::Url;

use crate::error::AppError;

use super::{Post, PostTags, Source};

const SITE: &str = "X";

/// 采集窗口只把这些 GraphQL 响应交给 Rust；返回它们来自哪种页面：用户的媒体（`media`），
/// 或自己的喜欢（`likes`）、书签（`bookmarks`）。媒体归采集页，喜欢和书签归收藏页。
pub fn timeline_kind(path: &str) -> Option<&'static str> {
    let path = path.split('?').next().unwrap_or(path);
    let path = path.strip_prefix("/i/api").unwrap_or(path);
    if !path.starts_with("/graphql/") {
        return None;
    }
    match path.rsplit('/').next()? {
        "UserMedia" | "UserTweets" | "TweetDetail" | "TweetResultByRestId" => Some("media"),
        "Likes" => Some("likes"),
        "Bookmarks" => Some("bookmarks"),
        _ => None,
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

pub fn parse_posts(body: &str) -> Result<Vec<Post>, AppError> {
    let value: Value = serde_json::from_str(body).map_err(|err| AppError::Parse {
        site: SITE,
        detail: err.to_string(),
    })?;
    let mut posts = Vec::new();
    let mut seen = HashSet::new();
    walk(&value, &mut seen, &mut posts);
    Ok(posts)
}

fn walk(value: &Value, seen: &mut HashSet<u64>, posts: &mut Vec<Post>) {
    match value {
        Value::Array(values) => values.iter().for_each(|value| walk(value, seen, posts)),
        Value::Object(object) => {
            if let Some(legacy) = object.get("legacy").and_then(Value::as_object) {
                parse_tweet(legacy, object.get("core"), seen, posts);
            }
            object.values().for_each(|value| walk(value, seen, posts));
        }
        _ => {}
    }
}

fn parse_tweet(
    legacy: &serde_json::Map<String, Value>,
    core: Option<&Value>,
    seen: &mut HashSet<u64>,
    posts: &mut Vec<Post>,
) {
    let Some(tweet_id) = text(legacy.get("id_str").or_else(|| legacy.get("id"))) else {
        return;
    };
    let Some(media) = legacy
        .get("extended_entities")
        .and_then(|entities| entities.get("media"))
        .or_else(|| {
            legacy
                .get("entities")
                .and_then(|entities| entities.get("media"))
        })
        .and_then(Value::as_array)
    else {
        return;
    };

    let (screen_name, display_name) = user_name(core);
    let hashtags = legacy
        .get("entities")
        .and_then(|entities| entities.get("hashtags"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tag| text(tag.get("text")))
        .collect::<Vec<_>>();

    for media in media {
        if media.get("type").and_then(Value::as_str) != Some("photo") {
            continue;
        }
        let Some(media_url) = text(
            media
                .get("media_url_https")
                .or_else(|| media.get("media_url")),
        ) else {
            continue;
        };
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
            file_size: media
                .get("original_info")
                .and_then(|info| info.get("size_bytes"))
                .and_then(Value::as_u64),
            file_url: Some(file_url),
            sample_url: Some(sample_url),
            thumb_url: Some(thumb_url),
            created_at: text(legacy.get("created_at")),
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
        let posts = parse_posts(&body.to_string()).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].source, Source::X);
        assert_eq!(posts[0].tags.artist, ["artist"]);
        assert_eq!(posts[0].tags.general, ["art"]);
        assert!(posts[0].file_url.as_deref().unwrap().contains("name=orig"));
        assert_eq!((posts[0].width, posts[0].height), (1200, 800));
    }

    #[test]
    fn filters_timeline_paths() {
        assert_eq!(timeline_kind("/i/api/graphql/query/UserMedia"), Some("media"));
        assert_eq!(timeline_kind("/graphql/query/UserMedia"), Some("media"));
        assert_eq!(timeline_kind("/i/api/graphql/query/UserTweets?variables=%7B%7D"), Some("media"));
        assert_eq!(timeline_kind("/i/api/graphql/query/Likes"), Some("likes"));
        assert_eq!(timeline_kind("/i/api/graphql/query/Bookmarks?variables=%7B%7D"), Some("bookmarks"));
        assert_eq!(timeline_kind("/i/api/graphql/query/Notifications"), None);
        assert_eq!(label_id(12), "x-000000000000000c");
    }
}
