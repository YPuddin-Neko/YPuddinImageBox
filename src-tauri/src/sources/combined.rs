//! 聚合搜索：同样的条件同时搜几个站点，按所选排序把各站点的结果合成一列。
//!
//! 各站点按自己的顺序一批一批地取。一张图要等每个还没翻完的站点都翻过它的位置才显示，
//! 没轮到的先记在翻页位置里，下一页接着排。这样不管往下翻多少页，整列都按所选排序。

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::{timestamp, Post, Sort, Source};

/// 帖子在所选排序里的位置，大的排在前面。上传先后比上传时间（两个站点的 id 不能互相比），
/// 按分数、分辨率排时一样的新图在前。
pub type Rank = (i64, i64);

/// 能合在一起排的排序。能不能用还要看所选站点是不是都支持（见 [`Sort::term`](super::Sort::term)）。
pub fn can_merge(sort: Sort) -> bool {
    matches!(sort, Sort::Newest | Sort::Oldest | Sort::Score | Sort::Resolution)
}

/// 认不出上传时间时为空（按分数排时照样能排）。
pub fn rank(post: &Post, sort: Sort) -> Option<Rank> {
    let time = post.created_at.as_deref().and_then(timestamp::parse);
    match sort {
        Sort::Score => Some((post.score, time.unwrap_or(0))),
        Sort::Resolution => Some((i64::from(post.width) * i64::from(post.height), time.unwrap_or(0))),
        Sort::Oldest => time.map(|time| (-time, 0)),
        _ => time.map(|time| (time, 0)),
    }
}

/// 这批图里排得最靠后的位置。
pub fn lowest(posts: &[Post], sort: Sort) -> Option<Rank> {
    posts.iter().filter_map(|post| rank(post, sort)).min()
}

/// 一个还没翻完的站点。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Site {
    pub source: Source,
    /// 下一页的位置；为空表示还没搜过。
    pub page: Option<String>,
    /// 已经取到的图里排得最靠后的位置，这个站点后面的图都排在它后面。还没取到图时为空。
    pub reached: Option<Rank>,
}

impl Site {
    /// 又取到一批图，最靠后的位置往后移。
    pub fn reach(&mut self, rank: Option<Rank>) {
        self.reached = match (self.reached, rank) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
}

/// 翻页位置：还没翻完的站点，加上已经取到、还没轮到显示的图（按各站点原来的顺序）。
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cursor {
    pub sites: Vec<Site>,
    pub held: Vec<Post>,
}

impl Cursor {
    pub fn start(sources: &[Source]) -> Cursor {
        let sites = sources.iter().map(|&source| Site { source, page: None, reached: None }).collect();
        Cursor { sites, held: Vec::new() }
    }

    /// 这一页要搜的站点：没翻完、也没有图在等着显示的。
    pub fn due(&self) -> Vec<&Site> {
        self.sites.iter().filter(|site| !self.held.iter().any(|post| post.source == site.source)).collect()
    }

    /// 还没翻完的站点里翻到的最靠前的位置。它们后面还可能有排在这之前的图，所以只显示排在这里或之前的图。
    pub fn bar(&self) -> Option<Rank> {
        self.sites.iter().filter_map(|site| site.reached).max()
    }

    pub fn is_done(&self) -> bool {
        self.sites.is_empty() && self.held.is_empty()
    }

    /// 一个站点这一页取完了：记下下一页和翻到的位置；`next` 为空表示翻完了。
    pub fn advance(&mut self, source: Source, next: Option<String>, reached: Option<Rank>) {
        match next {
            Some(page) => {
                if let Some(site) = self.sites.iter_mut().find(|site| site.source == source) {
                    site.page = Some(page);
                    site.reach(reached);
                }
            }
            None => self.stop(source),
        }
    }

    /// 这个站点不再往下翻（翻完了，或者账号、条件不对）。
    pub fn stop(&mut self, source: Source) {
        self.sites.retain(|site| site.source != source);
    }
}

/// 按站点分成几列（和 `sources` 的顺序一致），每列保持原来的顺序；不在 `sources` 里的图丢掉。
pub fn by_site(sources: &[Source], posts: Vec<Post>) -> Vec<VecDeque<Post>> {
    let mut queues: Vec<VecDeque<Post>> = sources.iter().map(|_| VecDeque::new()).collect();
    for post in posts {
        if let Some(index) = sources.iter().position(|source| *source == post.source) {
            queues[index].push_back(post);
        }
    }
    queues
}

/// 把各站点的图按排序合成一列，取出排在 `bar` 或之前的图，其余的按原来的顺序留在各自的列里；
/// `bar` 为空时全部取出。每列保持站点给的顺序，位置一样时先到的列在前。
/// 认不出位置的图轮到时直接取出，免得卡住它后面的图。
pub fn take_ready(queues: &mut [VecDeque<Post>], bar: Option<Rank>, sort: Sort) -> Vec<Post> {
    let mut ready = Vec::new();
    loop {
        let mut best: Option<(usize, Option<Rank>)> = None;
        for (index, queue) in queues.iter().enumerate() {
            let Some(post) = queue.front() else { continue };
            let rank = rank(post, sort);
            let ahead = match best {
                None => true,
                Some((_, current)) => match (rank, current) {
                    (None, Some(_)) => true,
                    (Some(rank), Some(current)) => rank > current,
                    _ => false,
                },
            };
            if ahead {
                best = Some((index, rank));
            }
        }
        let Some((index, rank)) = best else { break };
        if rank.zip(bar).is_some_and(|(rank, bar)| rank < bar) {
            break;
        }
        ready.extend(queues[index].pop_front());
    }
    ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::PostTags;

    /// 测试里合并这两个站点。
    const SITES: [Source; 2] = [Source::Danbooru, Source::Gelbooru];

    fn post(source: Source, id: u64, created_at: &str, score: i64) -> Post {
        Post {
            source,
            id,
            md5: None,
            width: 1,
            height: 1,
            rating: None,
            score,
            fav_count: None,
            file_ext: "jpg".into(),
            file_size: None,
            file_url: None,
            sample_url: None,
            thumb_url: None,
            created_at: Some(created_at.into()),
            post_url: String::new(),
            tags: PostTags::default(),
            pages: None,
        }
    }

    fn at(source: Source, id: u64, hour: u32) -> Post {
        post(source, id, &format!("2026-09-27T{hour:02}:00:00.000+00:00"), 0)
    }

    fn ids(posts: &[Post]) -> Vec<u64> {
        posts.iter().map(|post| post.id).collect()
    }

    /// 一个站点取到一批图，像命令里那样放进它的列，记下翻到的位置；`more` 为假表示这个站点翻完了。
    fn fetched(cursor: &mut Cursor, queues: &mut [VecDeque<Post>], posts: Vec<Post>, more: bool, sort: Sort) {
        let source = posts[0].source;
        let next = more.then(|| "2".to_string());
        cursor.advance(source, next, lowest(&posts, sort));
        queues[SITES.iter().position(|s| *s == source).unwrap()].extend(posts);
    }

    fn due(cursor: &Cursor) -> Vec<Source> {
        cursor.due().iter().map(|site| site.source).collect()
    }

    #[test]
    fn ranks_compare_upload_time_across_sites() {
        // Danbooru 的时间是 ISO 8601，Gelbooru 是另一种写法，要换成同一个时间轴再比。
        let danbooru = post(Source::Danbooru, 900, "2026-09-27T10:00:00.000+09:00", 5);
        let gelbooru = post(Source::Gelbooru, 70, "Sat Sep 26 20:30:00 -0500 2026", 5);
        // 换成 UTC：G#70 01:30，D#900 01:00。
        assert!(rank(&gelbooru, Sort::Newest) > rank(&danbooru, Sort::Newest));
        assert!(rank(&gelbooru, Sort::Oldest) < rank(&danbooru, Sort::Oldest));
        // 分数一样时新的在前。
        assert!(rank(&gelbooru, Sort::Score) > rank(&danbooru, Sort::Score));
        let mut unknown = danbooru.clone();
        unknown.created_at = Some("yesterday".into());
        assert_eq!(rank(&unknown, Sort::Newest), None);
        assert_eq!(rank(&unknown, Sort::Score), Some((5, 0)));
        let mut big = gelbooru.clone();
        (big.width, big.height) = (4000, 3000);
        assert!(rank(&big, Sort::Resolution) > rank(&danbooru, Sort::Resolution));
    }

    #[test]
    fn keeps_order_across_pages_when_one_site_is_busier() {
        // Gelbooru 一批只覆盖一两个小时，Danbooru 一批覆盖六个小时。
        let (d, g, sort) = (Source::Danbooru, Source::Gelbooru, Sort::Newest);
        let mut cursor = Cursor::start(&SITES);
        let mut queues = by_site(&SITES, Vec::new());
        fetched(&mut cursor, &mut queues, vec![at(d, 100, 23), at(d, 99, 20), at(d, 98, 17)], true, sort);
        fetched(&mut cursor, &mut queues, vec![at(g, 50, 23), at(g, 49, 22), at(g, 48, 21)], true, sort);
        // G 翻到 21 点，D 早于 21 点的图要等 G 再往下翻。
        let first = take_ready(&mut queues, cursor.bar(), sort);
        assert_eq!(ids(&first), [100, 50, 49, 48]);
        cursor.held = queues.into_iter().flatten().collect();
        assert_eq!(ids(&cursor.held), [99, 98]);
        // 下一页只搜没有图在等的 Gelbooru。
        assert_eq!(due(&cursor), [g]);

        let mut queues = by_site(&SITES, std::mem::take(&mut cursor.held));
        fetched(&mut cursor, &mut queues, vec![at(g, 47, 20), at(g, 46, 19), at(g, 45, 18)], false, sort);
        // G 翻完了，只剩 D 在限制：D 已经翻到 17 点，所以都能显示。
        let second = take_ready(&mut queues, cursor.bar(), sort);
        assert_eq!(ids(&second), [99, 47, 46, 45, 98]);
        assert!(queues.iter().all(VecDeque::is_empty));
        assert_eq!(due(&cursor), [d]);
    }

    fn keys(posts: &[Post]) -> Vec<(Source, u64)> {
        posts.iter().map(|post| (post.source, post.id)).collect()
    }

    #[test]
    fn oldest_and_score_merge_the_same_way() {
        let (d, g) = (Source::Danbooru, Source::Gelbooru);
        // 从旧到新：D 翻到 5 点、G 翻到 3 点，两边都没翻完，3 点之后的要等。
        let mut cursor = Cursor::start(&SITES);
        let mut queues = by_site(&SITES, Vec::new());
        fetched(&mut cursor, &mut queues, vec![at(d, 1, 1), at(d, 2, 5)], true, Sort::Oldest);
        fetched(&mut cursor, &mut queues, vec![at(g, 1, 2), at(g, 2, 3)], true, Sort::Oldest);
        let ready = take_ready(&mut queues, cursor.bar(), Sort::Oldest);
        assert_eq!(keys(&ready), [(d, 1), (g, 1), (g, 2)]);
        assert_eq!(queues[0].len(), 1);

        // 两边都翻完了：全部按分数排，同样 9 分时新的在前。
        let score = |source, id, hour, score| post(source, id, &format!("2026-09-27T{hour:02}:00:00Z"), score);
        let posts = vec![score(d, 1, 1, 50), score(d, 2, 1, 9), score(g, 1, 2, 9), score(g, 2, 2, 3)];
        let mut queues = by_site(&SITES, posts);
        let ready = take_ready(&mut queues, None, Sort::Score);
        assert_eq!(keys(&ready), [(d, 1), (g, 1), (d, 2), (g, 2)]);
    }

    #[test]
    fn unknown_times_do_not_block_the_queue() {
        let (d, g) = (Source::Danbooru, Source::Gelbooru);
        let mut odd = at(d, 7, 12);
        odd.created_at = None;
        let mut queues = by_site(&SITES, vec![odd, at(d, 6, 11), at(g, 1, 20)]);
        // 认不出时间的 D#7 轮到就放出；G#1 正好在界线（20 点）上也放出；D#6 在 11 点，要等。
        let ready = take_ready(&mut queues, rank(&at(g, 1, 20), Sort::Newest), Sort::Newest);
        assert_eq!(keys(&ready), [(d, 7), (g, 1)]);
        assert_eq!(ids(&queues[0].iter().cloned().collect::<Vec<_>>()), [6]);
    }

    #[test]
    fn cursor_round_trips_through_json() {
        let mut cursor = Cursor::start(&SITES);
        cursor.sites[0].page = Some("b123".into());
        cursor.sites[0].reach(Some((5, 0)));
        cursor.sites[0].reach(Some((9, 0)));
        cursor.held.push(at(Source::Danbooru, 3, 4));
        let text = serde_json::to_string(&cursor).unwrap();
        let back: Cursor = serde_json::from_str(&text).unwrap();
        assert_eq!(back.sites, cursor.sites);
        assert_eq!(back.sites[0].reached, Some((5, 0)));
        assert_eq!(ids(&back.held), [3]);
        assert!(!back.is_done());
        assert!(Cursor::default().is_done());
    }
}
