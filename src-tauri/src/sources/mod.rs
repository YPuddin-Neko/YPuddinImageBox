//! 各 booru 站点的适配器：把不同站点的帖子统一成 [`Post`]。

pub mod danbooru;
pub mod gelbooru;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Danbooru,
    Gelbooru,
}

impl Source {
    pub fn site_name(self) -> &'static str {
        match self {
            Source::Danbooru => "Danbooru",
            Source::Gelbooru => "Gelbooru",
        }
    }

    /// 允许加载图片的域名（含子域名）。预览代理只放行这些域名。
    pub fn allowed_host_suffixes(self) -> &'static [&'static str] {
        match self {
            Source::Danbooru => &["donmai.us"],
            Source::Gelbooru => &["gelbooru.com"],
        }
    }

    pub fn referer(self) -> &'static str {
        match self {
            Source::Danbooru => "https://danbooru.donmai.us/",
            Source::Gelbooru => "https://gelbooru.com/",
        }
    }

    pub fn for_host(host: &str) -> Option<Source> {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        [Source::Danbooru, Source::Gelbooru].into_iter().find(|source| {
            source
                .allowed_host_suffixes()
                .iter()
                .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    General,
    Sensitive,
    Questionable,
    Explicit,
}

impl Rating {
    pub const ALL: [Rating; 4] = [Rating::General, Rating::Sensitive, Rating::Questionable, Rating::Explicit];

    /// Danbooru 用单字母（g/s/q/e），Gelbooru 用全称；旧数据里的 safe 按一般处理。
    pub fn parse(value: &str) -> Option<Rating> {
        match value.trim().to_ascii_lowercase().as_str() {
            "g" | "general" | "safe" => Some(Rating::General),
            "s" | "sensitive" => Some(Rating::Sensitive),
            "q" | "questionable" => Some(Rating::Questionable),
            "e" | "explicit" => Some(Rating::Explicit),
            _ => None,
        }
    }

    fn danbooru_code(self) -> &'static str {
        match self {
            Rating::General => "g",
            Rating::Sensitive => "s",
            Rating::Questionable => "q",
            Rating::Explicit => "e",
        }
    }

    fn gelbooru_name(self) -> &'static str {
        match self {
            Rating::General => "general",
            Rating::Sensitive => "sensitive",
            Rating::Questionable => "questionable",
            Rating::Explicit => "explicit",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostTags {
    pub artist: Vec<String>,
    pub copyright: Vec<String>,
    pub character: Vec<String>,
    pub general: Vec<String>,
    pub meta: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub source: Source,
    pub id: u64,
    pub md5: Option<String>,
    pub width: u32,
    pub height: u32,
    pub rating: Option<Rating>,
    pub score: i64,
    pub fav_count: Option<i64>,
    pub file_ext: String,
    pub file_size: Option<u64>,
    /// 原图。部分帖子对未登录用户隐藏原图，此时为空。
    pub file_url: Option<String>,
    /// 详情面板用的中等尺寸图。
    pub sample_url: Option<String>,
    /// 瀑布流缩略图。
    pub thumb_url: Option<String>,
    pub created_at: Option<String>,
    pub post_url: String,
    pub tags: PostTags,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchParams {
    pub source: Source,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub ratings: Vec<Rating>,
    #[serde(default = "first_page")]
    pub page: u32,
}

fn first_page() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub posts: Vec<Post>,
    pub page: u32,
    pub has_more: bool,
    /// 实际发给站点的查询串，界面上展示给用户核对。
    pub query: String,
}

/// 用户输入的 tag 加上分级条件，翻译成站点的查询语法。
/// 四个分级全选或都不选时不加分级条件。
pub fn build_query(source: Source, tags: &str, ratings: &[Rating]) -> String {
    let mut parts: Vec<String> = tags.split_whitespace().map(str::to_string).collect();
    let selected: Vec<Rating> = Rating::ALL.into_iter().filter(|r| ratings.contains(r)).collect();
    if !selected.is_empty() && selected.len() < Rating::ALL.len() {
        match source {
            Source::Danbooru => {
                let codes: Vec<&str> = selected.iter().map(|r| r.danbooru_code()).collect();
                parts.push(format!("rating:{}", codes.join(",")));
            }
            Source::Gelbooru if selected.len() == 1 => {
                parts.push(format!("rating:{}", selected[0].gelbooru_name()));
            }
            Source::Gelbooru => {
                let alternatives: Vec<String> =
                    selected.iter().map(|r| format!("rating:{}", r.gelbooru_name())).collect();
                parts.push(format!("{{{}}}", alternatives.join(" ~ ")));
            }
        }
    }
    parts.join(" ")
}

/// 空格分隔的 tag 串拆成列表，去空、去重、保序。
pub(crate) fn split_tags(value: Option<&str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    value
        .unwrap_or_default()
        .split_whitespace()
        .filter(|tag| seen.insert(tag.to_string()))
        .map(str::to_string)
        .collect()
}

/// 把空字符串当成缺失。
pub(crate) fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_adds_rating_per_site_syntax() {
        let ratings = [Rating::General, Rating::Sensitive];
        assert_eq!(build_query(Source::Danbooru, " 1girl  scenery ", &ratings), "1girl scenery rating:g,s");
        assert_eq!(
            build_query(Source::Gelbooru, "scenery", &ratings),
            "scenery {rating:general ~ rating:sensitive}"
        );
        assert_eq!(build_query(Source::Gelbooru, "scenery", &[Rating::General]), "scenery rating:general");
        assert_eq!(build_query(Source::Danbooru, "scenery", &Rating::ALL), "scenery");
        assert_eq!(build_query(Source::Danbooru, "", &[]), "");
    }

    #[test]
    fn host_matching_respects_dot_boundary() {
        assert_eq!(Source::for_host("cdn.donmai.us"), Some(Source::Danbooru));
        assert_eq!(Source::for_host("img4.gelbooru.com."), Some(Source::Gelbooru));
        assert_eq!(Source::for_host("evildonmai.us"), None);
        assert_eq!(Source::for_host("donmai.us.example.com"), None);
    }
}
