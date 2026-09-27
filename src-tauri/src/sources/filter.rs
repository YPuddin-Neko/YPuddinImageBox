//! 超出 tag 上限时的本地筛选。
//!
//! Danbooru 一次能搜的 tag 数有限（未登录 2 个，Gold 6 个，Platinum 以上 12 个；`rating:`、`date:`、`score:`
//! 不计数）。超出时前面几个普通 tag 交给站点搜，剩下的在本地按每个帖子的 tag 逐一筛选。
//! 带冒号的条件（order:、width: 等）只能交给站点，超出上限时照常报错。

use super::{build_query, Post, Rating, Source};
use crate::error::AppError;

/// 这些条件不计入 Danbooru 的 tag 数量。
const FREE_PREFIXES: [&str; 3] = ["rating:", "date:", "score:"];

/// 按账号等级给出 Danbooru 一次最多能搜几个 tag。等级未知时按未登录算。
pub fn danbooru_tag_limit(level: Option<&str>) -> usize {
    match level.map(str::to_ascii_lowercase).as_deref() {
        Some("gold") => 6,
        Some("platinum" | "builder" | "contributor" | "approver" | "moderator" | "admin" | "owner") => 12,
        _ => 2,
    }
}

/// 在本地筛选的 tag：帖子必须有全部 `include`，不能有任何 `exclude`。支持 `*` 通配。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalFilter {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

impl LocalFilter {
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// 从空格分隔的写法读回，`-tag` 表示排除。存进数据库时用 [`LocalFilter::to_query`]。
    pub fn parse(value: &str) -> Self {
        let mut filter = LocalFilter::default();
        for tag in value.split_whitespace().map(str::to_lowercase) {
            match tag.strip_prefix('-') {
                Some(name) if !name.is_empty() => filter.exclude.push(name.to_string()),
                _ => filter.include.push(tag),
            }
        }
        filter
    }

    pub fn to_query(&self) -> String {
        let excluded = self.exclude.iter().map(|tag| format!("-{tag}"));
        self.include.iter().cloned().chain(excluded).collect::<Vec<_>>().join(" ")
    }

    pub fn matches(&self, post: &Post) -> bool {
        let tags: Vec<&str> = post
            .tags
            .by_category()
            .into_iter()
            .flat_map(|(_, names)| names.iter().map(String::as_str))
            .collect();
        let has = |pattern: &str| tags.iter().any(|tag| glob(pattern, tag));
        self.include.iter().all(|pattern| has(pattern)) && !self.exclude.iter().any(|pattern| has(pattern))
    }
}

/// 只支持 `*` 的通配匹配。
fn glob(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else { return false };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else { return rest.is_empty() };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// 一次搜索怎么分：发给站点的查询串，以及留在本地筛选的 tag。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlan {
    pub server_query: String,
    pub local: LocalFilter,
}

/// 按站点的 tag 上限拆分用户输入。`limit` 为 `None` 表示没有上限（Gelbooru）。
pub fn plan_query(source: Source, tags: &str, ratings: &[Rating], limit: Option<usize>) -> Result<QueryPlan, AppError> {
    let words: Vec<String> = tags.split_whitespace().map(str::to_string).collect();
    let everything = QueryPlan { server_query: build_query(source, tags, ratings), local: LocalFilter::default() };
    let Some(limit) = limit else { return Ok(everything) };
    let counted = |tag: &&String| !FREE_PREFIXES.iter().any(|prefix| tag.to_ascii_lowercase().starts_with(prefix));
    if words.iter().filter(counted).count() <= limit {
        return Ok(everything);
    }
    // 带冒号的条件只能由站点处理；普通 tag 里先放要「有」的，排除项更适合留在本地。
    let (meta, plain): (Vec<&String>, Vec<&String>) =
        words.iter().filter(counted).partition(|tag| tag.trim_start_matches('-').contains(':'));
    if meta.len() > limit {
        return Err(AppError::TagLimit { site: source.site_name(), limit: limit as u32 });
    }
    let (positive, negative): (Vec<&String>, Vec<&String>) = plain.into_iter().partition(|tag| !tag.starts_with('-'));
    let room = limit - meta.len();
    let ordered: Vec<&String> = positive.into_iter().chain(negative).collect();
    let (to_server, to_local) = ordered.split_at(room.min(ordered.len()));

    let free: Vec<&String> = words.iter().filter(|tag| !counted(tag)).collect();
    let server_tags: Vec<&str> = meta.iter().chain(to_server).chain(free.iter()).map(|tag| tag.as_str()).collect();
    let local = LocalFilter::parse(&to_local.iter().map(|tag| tag.as_str()).collect::<Vec<_>>().join(" "));
    Ok(QueryPlan { server_query: build_query(source, &server_tags.join(" "), ratings), local })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::PostTags;

    fn post(general: &[&str], artist: &[&str]) -> Post {
        Post {
            source: Source::Danbooru,
            id: 1,
            md5: None,
            width: 1,
            height: 1,
            rating: None,
            score: 0,
            fav_count: None,
            file_ext: "png".into(),
            file_size: None,
            file_url: None,
            sample_url: None,
            thumb_url: None,
            created_at: None,
            post_url: String::new(),
            tags: PostTags {
                general: general.iter().map(|s| s.to_string()).collect(),
                artist: artist.iter().map(|s| s.to_string()).collect(),
                ..PostTags::default()
            },
        }
    }

    #[test]
    fn limits_follow_account_level() {
        assert_eq!(danbooru_tag_limit(None), 2);
        assert_eq!(danbooru_tag_limit(Some("Member")), 2);
        assert_eq!(danbooru_tag_limit(Some("Gold")), 6);
        assert_eq!(danbooru_tag_limit(Some("Platinum")), 12);
        assert_eq!(danbooru_tag_limit(Some("Builder")), 12);
    }

    #[test]
    fn within_limit_goes_to_server_unchanged() {
        let plan = plan_query(Source::Danbooru, "scenery sky", &[Rating::General], Some(2)).unwrap();
        assert_eq!(plan.server_query, "scenery sky rating:g");
        assert!(plan.local.is_empty());
        // rating:、score: 不计数。
        let plan = plan_query(Source::Danbooru, "scenery sky score:>10", &[], Some(2)).unwrap();
        assert!(plan.local.is_empty());
        // Gelbooru 没有上限。
        assert!(plan_query(Source::Gelbooru, "a b c d e", &[], None).unwrap().local.is_empty());
    }

    #[test]
    fn extra_tags_are_filtered_locally() {
        let plan = plan_query(Source::Danbooru, "scenery -comic sky cloud score:>5", &[Rating::General], Some(2)).unwrap();
        // 先放要「有」的 tag，排除项留在本地；不计数的条件照常发给站点。
        assert_eq!(plan.server_query, "scenery sky score:>5 rating:g");
        assert_eq!(plan.local, LocalFilter { include: vec!["cloud".into()], exclude: vec!["comic".into()] });
        assert_eq!(plan.local.to_query(), "cloud -comic");
        assert_eq!(LocalFilter::parse(&plan.local.to_query()), plan.local);
    }

    #[test]
    fn meta_tags_must_fit_the_server() {
        let plan = plan_query(Source::Danbooru, "order:score scenery sky", &[], Some(2)).unwrap();
        assert_eq!(plan.server_query, "order:score scenery");
        assert_eq!(plan.local.include, vec!["sky".to_string()]);
        let err = plan_query(Source::Danbooru, "order:score width:>1000 filesize:..1mb", &[], Some(2)).unwrap_err();
        assert!(matches!(err, AppError::TagLimit { limit: 2, .. }));
    }

    #[test]
    fn local_matching_checks_every_category_and_wildcards() {
        let p = post(&["cloud", "blue_sky"], &["kantoku"]);
        assert!(LocalFilter::parse("cloud kantoku").matches(&p));
        assert!(LocalFilter::parse("*_sky").matches(&p));
        assert!(LocalFilter::parse("kan*ku").matches(&p));
        assert!(!LocalFilter::parse("cloud -kantoku").matches(&p));
        assert!(!LocalFilter::parse("rain").matches(&p));
        assert!(LocalFilter::parse("-rain").matches(&p));
        assert!(glob("a*b*c", "axxbyyc") && !glob("a*b*c", "axxbyy") && glob("*", "x") && !glob("ab", "abc"));
    }

    #[test]
    fn gold_only_posts_are_danbooru_posts_with_restricted_tags() {
        assert!(post(&["loli", "sky"], &[]).gold_only());
        assert!(!post(&["sky"], &[]).gold_only());
        let gelbooru = Post { source: Source::Gelbooru, ..post(&["loli"], &[]) };
        assert!(!gelbooru.gold_only());
    }
}
