//! 本地图库和下载任务的数据库（SQLite，放在「数据库」位置下）。
//!
//! 帖子按（来源，站点 id）唯一。tag 按名称合并，两个站点共用；Gelbooru 的 tag 不带分类，
//! 所以已经有具体分类的 tag 不会被改回「一般」。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::i18n::{text, tr};
use crate::sources::{join_sources, split_sources, timestamp, Post, PostTags, Rating, Sort, Source};

const DB_FILE: &str = "library.sqlite3";
/// 一次列表查询最多返回多少张。
const MAX_PAGE: u32 = 200;

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Clone)]
pub struct Library {
    pool: SqlitePool,
}

/// 图库里的一张图：站点信息加上本地文件。
/// 缩略图和详情图的地址指向本地路由，界面和远程帖子用同一套加载方式。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPost {
    #[serde(flatten)]
    pub post: Post,
    pub path: String,
    pub downloaded_at: i64,
    /// 文件已经不在记录的位置（被移动或删除）。列表返回前由调用方检查。
    pub missing: bool,
}

/// 收藏的搜索条件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSearch {
    pub id: i64,
    /// 搜哪些站点，按固定顺序；有两个以上时是聚合搜索。
    pub sources: Vec<Source>,
    pub tags: String,
    /// 为空表示全选。
    pub ratings: Vec<Rating>,
    pub sort: Sort,
    pub created_at: i64,
}

/// 存进数据库前整理条件：tag 去掉多余空格，分级按固定顺序，全选和都不选都记成空。
fn normalize_search(tags: &str, ratings: &[Rating]) -> (String, String) {
    let tags = tags.split_whitespace().collect::<Vec<_>>().join(" ");
    let chosen: Vec<&str> = Rating::ALL.into_iter().filter(|r| ratings.contains(r)).map(Rating::as_str).collect();
    let ratings = if chosen.len() == Rating::ALL.len() { String::new() } else { chosen.join(",") };
    (tags, ratings)
}

/// 图库的排序。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LibrarySort {
    /// 最近下载的在前。
    #[default]
    Downloaded,
    DownloadedAsc,
    /// 按帖子的发布时间，新的在前。
    Newest,
    Oldest,
    Score,
    Favorites,
    Resolution,
    Filesize,
}

impl LibrarySort {
    /// ORDER BY 子句。缺少数据的排在最后；最后都按 id 兜底，翻页时顺序稳定。
    fn order_by(self) -> &'static str {
        match self {
            LibrarySort::Downloaded => "p.downloaded_at DESC, p.id DESC",
            LibrarySort::DownloadedAsc => "p.downloaded_at ASC, p.id ASC",
            LibrarySort::Newest => "p.posted_at DESC NULLS LAST, p.id DESC",
            LibrarySort::Oldest => "p.posted_at ASC NULLS LAST, p.id ASC",
            LibrarySort::Score => "p.score DESC, p.id DESC",
            LibrarySort::Favorites => "p.fav_count DESC NULLS LAST, p.id DESC",
            LibrarySort::Resolution => "p.width * p.height DESC, p.id DESC",
            LibrarySort::Filesize => "p.file_size DESC NULLS LAST, p.id DESC",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryQuery {
    #[serde(default)]
    pub source: Option<Source>,
    /// 空格分隔；每个 tag 都要有，`-tag` 表示排除。
    #[serde(default)]
    pub tags: String,
    /// 为空或全选时不按分级筛选。
    #[serde(default)]
    pub ratings: Vec<Rating>,
    #[serde(default)]
    pub sort: LibrarySort,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPage {
    pub posts: Vec<LocalPost>,
    pub total: i64,
    pub offset: u32,
    pub has_more: bool,
}

/// 文件夹和分组卡片上扇形展开的封面，按下载时间从新到旧。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cover {
    pub source: Source,
    pub post_id: u64,
    pub width: u32,
    pub height: u32,
    /// 本地缩略图的路由，和图库列表里的 thumbUrl 一样。
    pub thumb_url: String,
}

/// 图库首页按来源分的文件夹。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub source: Source,
    pub count: i64,
    /// 最近一次下载的时间，文件夹是空的时为空。
    pub latest_at: Option<i64>,
    pub covers: Vec<Cover>,
}

/// 文件夹里按哪类 tag 分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
    Artist,
    Copyright,
    Character,
    General,
}

impl GroupKind {
    fn category(self) -> &'static str {
        match self {
            GroupKind::Artist => "artist",
            GroupKind::Copyright => "copyright",
            GroupKind::Character => "character",
            GroupKind::General => "general",
        }
    }
}

/// 分组的排序；默认最近有新下载的在前。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupSort {
    #[default]
    Recent,
    Count,
    Name,
}

impl GroupSort {
    fn order_by(self) -> &'static str {
        match self {
            GroupSort::Recent => "latest DESC, t.id DESC",
            GroupSort::Count => "count DESC, latest DESC",
            GroupSort::Name => "t.name",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupQuery {
    pub source: Source,
    pub kind: GroupKind,
    #[serde(default)]
    pub sort: GroupSort,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub limit: u32,
}

/// 一个分组：同一个画师（作品、角色、tag）的图。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub name: String,
    pub count: i64,
    pub latest_at: i64,
    pub covers: Vec<Cover>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupPage {
    pub groups: Vec<Group>,
    /// 这个文件夹里一共有多少组。
    pub total: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    /// 选中的帖子。
    Posts,
    /// 按条件下载全部结果，边翻页边下载。
    Query,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Pending,
    Saved,
    Skipped,
    Failed,
}

macro_rules! text_enum {
    ($ty:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl $ty {
            pub fn as_str(self) -> &'static str {
                match self { $($ty::$variant => $text),+ }
            }

            fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some($ty::$variant),)+ _ => None }
            }
        }
    };
}

text_enum!(JobKind { Posts => "posts", Query => "query" });
text_enum!(JobStatus {
    Queued => "queued",
    Running => "running",
    Paused => "paused",
    Done => "done",
    Failed => "failed",
    Canceled => "canceled",
});
text_enum!(ItemStatus { Pending => "pending", Saved => "saved", Skipped => "skipped", Failed => "failed" });

/// 界面上显示的任务。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: i64,
    pub kind: JobKind,
    pub source: Source,
    pub title: String,
    pub query: Option<String>,
    pub max_posts: Option<i64>,
    pub status: JobStatus,
    /// 总张数。按条件下载时先是站点给的估计值，翻完后改成实际张数。
    pub total: Option<i64>,
    pub saved: i64,
    pub skipped: i64,
    pub failed: i64,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// 按条件下载时还有没有下一页。
    #[serde(skip)]
    pub cursor: Option<String>,
    /// 由订阅检查生成时，对应的订阅。
    pub subscription_id: Option<i64>,
    /// 超出 tag 上限、在本地筛选的 tag。
    pub local_filter: Option<String>,
}

/// 订阅：按设定的间隔检查条件下有没有新图，有就自动下载。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: i64,
    pub source: Source,
    /// 用户填写的 tag（不含分级）。
    pub tags: String,
    pub ratings: Vec<Rating>,
    /// 发给站点的查询串。
    pub query: String,
    pub enabled: bool,
    pub interval_minutes: i64,
    /// 已经处理到的最大帖子 id，比它新的才算新图。
    pub last_seen_id: i64,
    pub last_checked_at: Option<i64>,
    /// 最近一次检查找到的新图张数。
    pub last_new: i64,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// 还在排队、下载或暂停中的检查任务。
    pub active_job: Option<i64>,
    /// 超出 tag 上限、在本地筛选的 tag。
    pub local_filter: Option<String>,
}

impl Subscription {
    /// 界面和任务列表里显示的名字。
    pub fn title(&self) -> String {
        if self.tags.trim().is_empty() {
            text("全部帖子", "All posts").into()
        } else {
            self.tags.clone()
        }
    }
}

pub struct NewSubscription<'a> {
    pub source: Source,
    pub tags: &'a str,
    pub ratings: &'a [Rating],
    pub query: &'a str,
    pub interval_minutes: i64,
    pub last_seen_id: i64,
    pub local_filter: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct JobItem {
    pub seq: i64,
    pub post: Post,
}

/// 失败或跳过的原因，界面在任务详情里列出。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemNote {
    pub post_id: i64,
    pub status: &'static str,
    pub note: Option<String>,
}

fn db_err(message: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(message.into())
}

fn unknown_source(source: &str) -> sqlx::Error {
    db_err(tr!("未知的来源 {source}", "Unknown source {source}"))
}

/// 刚写进去的任务读不出来。
fn job_missing() -> sqlx::Error {
    db_err(tr!("任务写入后读取失败", "Couldn't read the job back after saving it"))
}

fn job_from_row(row: &SqliteRow) -> Result<JobInfo, sqlx::Error> {
    let kind: String = row.try_get("kind")?;
    let source: String = row.try_get("source")?;
    let status: String = row.try_get("status")?;
    Ok(JobInfo {
        id: row.try_get("id")?,
        kind: JobKind::parse(&kind).ok_or_else(|| db_err(tr!("未知的任务类型 {kind}", "Unknown job kind {kind}")))?,
        source: Source::parse(&source).ok_or_else(|| unknown_source(&source))?,
        title: row.try_get("title")?,
        query: row.try_get("query")?,
        max_posts: row.try_get("max_posts")?,
        status: JobStatus::parse(&status)
            .ok_or_else(|| db_err(tr!("未知的任务状态 {status}", "Unknown job status {status}")))?,
        total: row.try_get("total")?,
        saved: row.try_get("saved")?,
        skipped: row.try_get("skipped")?,
        failed: row.try_get("failed")?,
        error: row.try_get("error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        cursor: row.try_get("cursor")?,
        subscription_id: row.try_get("subscription_id")?,
        local_filter: row.try_get("local_filter")?,
    })
}

fn subscription_from_row(row: &SqliteRow) -> Result<Subscription, sqlx::Error> {
    let source: String = row.try_get("source")?;
    let ratings: String = row.try_get("ratings")?;
    Ok(Subscription {
        id: row.try_get("id")?,
        source: Source::parse(&source).ok_or_else(|| unknown_source(&source))?,
        tags: row.try_get("tags")?,
        ratings: ratings.split(',').filter_map(Rating::parse).collect(),
        query: row.try_get("query")?,
        enabled: row.try_get("enabled")?,
        interval_minutes: row.try_get("interval_minutes")?,
        last_seen_id: row.try_get("last_seen_id")?,
        last_checked_at: row.try_get("last_checked_at")?,
        last_new: row.try_get("last_new")?,
        last_error: row.try_get("last_error")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        active_job: row.try_get("active_job")?,
        local_filter: row.try_get("local_filter")?,
    })
}

/// 图库里的图经 ibx 协议加载的路由：`kind` 是 thumb（缩略图）或 file（原图）。
fn local_route(kind: &str, source: Source, post_id: u64) -> String {
    format!("local/{kind}/{}/{post_id}", source.as_str())
}

/// 每个文件夹、分组卡片上最多放几张封面。
const COVERS: i64 = 5;

fn cover(source: Source, post_id: i64, width: u32, height: u32) -> Cover {
    let post_id = post_id as u64;
    Cover { source, post_id, width, height, thumb_url: local_route("thumb", source, post_id) }
}

fn post_from_row(row: &SqliteRow) -> Result<LocalPost, sqlx::Error> {
    let source: String = row.try_get("source")?;
    let source = Source::parse(&source).ok_or_else(|| unknown_source(&source))?;
    let post_id: i64 = row.try_get("post_id")?;
    let rating: Option<String> = row.try_get("rating")?;
    let file_size: Option<i64> = row.try_get("file_size")?;
    let route = |kind: &str| Some(local_route(kind, source, post_id as u64));
    Ok(LocalPost {
        post: Post {
            source,
            id: post_id as u64,
            md5: row.try_get("md5")?,
            width: row.try_get("width")?,
            height: row.try_get("height")?,
            rating: rating.as_deref().and_then(Rating::parse),
            score: row.try_get("score")?,
            fav_count: row.try_get("fav_count")?,
            file_ext: row.try_get("file_ext")?,
            file_size: file_size.map(|size| size as u64),
            file_url: row.try_get("file_url")?,
            sample_url: route("file"),
            thumb_url: route("thumb"),
            created_at: row.try_get("created_at")?,
            post_url: row.try_get("post_url")?,
            tags: PostTags::default(),
            pages: None,
        },
        path: row.try_get("local_path")?,
        downloaded_at: row.try_get("downloaded_at")?,
        missing: false,
    })
}

const JOB_COLUMNS: &str = "id, kind, source, title, query, max_posts, status, total, saved, skipped, failed, \
                           cursor, error, created_at, updated_at, subscription_id, local_filter";

const SUBSCRIPTION_COLUMNS: &str = "s.id, s.source, s.tags, s.ratings, s.query, s.enabled, s.interval_minutes, \
     s.last_seen_id, s.last_checked_at, s.last_new, s.last_error, s.created_at, s.updated_at, s.local_filter, \
     (SELECT j.id FROM jobs j WHERE j.subscription_id = s.id AND j.status IN ('queued', 'running', 'paused') \
      ORDER BY j.id DESC LIMIT 1) AS active_job";

impl Library {
    pub async fn open(dir: &Path) -> Result<Self, sqlx::Error> {
        std::fs::create_dir_all(dir)?;
        let options = SqliteConnectOptions::new()
            .filename(dir.join(DB_FILE))
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new().max_connections(4).connect_with(options).await?;
        Self::migrate(pool).await
    }

    /// 测试用的内存数据库。内存库每个连接各是一份，所以只开一个连接。
    #[cfg(test)]
    pub async fn in_memory() -> Self {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.unwrap();
        Self::migrate(pool).await.unwrap()
    }

    async fn migrate(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        sqlx::migrate!("./migrations").run(&pool).await?;
        let library = Self { pool };
        library.backfill_posted_at().await?;
        library.optimize().await?;
        Ok(library)
    }

    /// 更新查询优化器的统计信息。图库涨到几万张后没有统计信息，SQLite 会挑错索引，
    /// 例如按来源筛选时放弃按时间的索引、把几万行取出来再排序。只分析从没分析过或变化很大的表。
    /// 不抽样：抽样时来源、分级这种只有几个取值的列会被当成区分度很高，照样挑错。
    pub async fn optimize(&self) -> Result<(), sqlx::Error> {
        sqlx::raw_sql("PRAGMA optimize = 0x10002;").execute(&self.pool).await?;
        Ok(())
    }

    /// 诊断用：SQLite 版本，以及是否已经有统计信息。
    pub async fn stats_info(&self) -> Result<(String, bool), sqlx::Error> {
        let version: String = sqlx::query_scalar("SELECT sqlite_version()").fetch_one(&self.pool).await?;
        let analyzed: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'sqlite_stat1')")
                .fetch_one(&self.pool)
                .await?;
        Ok((version, analyzed))
    }

    /// 补上发布时间：加这一列之前下载的图只有站点原样的 created_at。认不出格式的留空，排序时放在最后。
    async fn backfill_posted_at(&self) -> Result<(), sqlx::Error> {
        let rows: Vec<(i64, String)> =
            sqlx::query_as("SELECT id, created_at FROM posts WHERE posted_at IS NULL AND created_at IS NOT NULL")
                .fetch_all(&self.pool)
                .await?;
        let parsed: Vec<(i64, i64)> =
            rows.into_iter().filter_map(|(id, created)| Some((id, timestamp::parse(&created)?))).collect();
        if parsed.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for (id, posted_at) in parsed {
            sqlx::query("UPDATE posts SET posted_at = ? WHERE id = ?").bind(posted_at).bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await
    }

    // ---------- 图库 ----------

    /// 已在图库里的帖子的本地路径。
    pub async fn local_path(&self, source: Source, post_id: u64) -> Result<Option<PathBuf>, sqlx::Error> {
        let path: Option<String> = sqlx::query_scalar("SELECT local_path FROM posts WHERE source = ? AND post_id = ?")
            .bind(source.as_str())
            .bind(post_id as i64)
            .fetch_optional(&self.pool)
            .await?;
        Ok(path.map(PathBuf::from))
    }

    /// 图库里 md5 相同的其他帖子（同一张图在另一个站点或另一个帖子里）的本地路径。
    pub async fn paths_with_md5(&self, md5: &str, source: Source, post_id: u64) -> Result<Vec<PathBuf>, sqlx::Error> {
        let paths: Vec<String> = sqlx::query_scalar(
            "SELECT local_path FROM posts WHERE md5 = ? AND NOT (source = ? AND post_id = ?)",
        )
        .bind(md5.to_ascii_lowercase())
        .bind(source.as_str())
        .bind(post_id as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(paths.into_iter().map(PathBuf::from).collect())
    }

    /// 给出的帖子里哪些已经在图库中：这个帖子下载过，或者同一张图（md5 相同）从别的帖子、别的站点下载过。
    pub async fn owned(&self, posts: &[Post]) -> Result<HashSet<(Source, u64)>, sqlx::Error> {
        if posts.is_empty() {
            return Ok(HashSet::new());
        }
        let mut query = QueryBuilder::<Sqlite>::new("SELECT source, post_id, md5 FROM posts WHERE ");
        let mut first = true;
        for source in Source::ALL {
            let ids: Vec<i64> = posts.iter().filter(|post| post.source == source).map(|post| post.id as i64).collect();
            if ids.is_empty() {
                continue;
            }
            query.push(if first { "(source = " } else { " OR (source = " });
            first = false;
            query.push_bind(source.as_str()).push(" AND post_id IN (");
            let mut list = query.separated(", ");
            for id in ids {
                list.push_bind(id);
            }
            query.push("))");
        }
        let md5s: Vec<String> = posts.iter().filter_map(|post| post.md5.as_deref()).map(str::to_ascii_lowercase).collect();
        if !md5s.is_empty() {
            query.push(" OR md5 IN (");
            let mut list = query.separated(", ");
            for md5 in md5s {
                list.push_bind(md5);
            }
            query.push(")");
        }
        let rows: Vec<(String, i64, Option<String>)> = query.build_query_as().fetch_all(&self.pool).await?;
        let saved: HashSet<(&str, i64)> = rows.iter().map(|(source, id, _)| (source.as_str(), *id)).collect();
        let hashes: HashSet<&str> = rows.iter().filter_map(|(_, _, md5)| md5.as_deref()).collect();
        Ok(posts
            .iter()
            .filter(|post| {
                saved.contains(&(post.source.as_str(), post.id as i64))
                    || post.md5.as_deref().is_some_and(|md5| hashes.contains(md5.to_ascii_lowercase().as_str()))
            })
            .map(|post| (post.source, post.id))
            .collect())
    }

    /// 记录一张下载好的图。重复下载同一帖子时更新信息和路径。
    pub async fn save_post(&self, post: &Post, path: &Path, downloaded_at: i64) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO posts (source, post_id, md5, width, height, rating, score, fav_count, file_ext, file_size,
                                file_url, created_at, posted_at, post_url, local_path, downloaded_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (source, post_id) DO UPDATE SET
                md5 = excluded.md5, width = excluded.width, height = excluded.height, rating = excluded.rating,
                score = excluded.score, fav_count = excluded.fav_count, file_ext = excluded.file_ext,
                file_size = excluded.file_size, file_url = excluded.file_url, created_at = excluded.created_at,
                posted_at = excluded.posted_at, post_url = excluded.post_url, local_path = excluded.local_path,
                downloaded_at = excluded.downloaded_at
             RETURNING id",
        )
        .bind(post.source.as_str())
        .bind(post.id as i64)
        .bind(post.md5.as_deref().map(str::to_ascii_lowercase))
        .bind(post.width)
        .bind(post.height)
        .bind(post.rating.map(Rating::as_str))
        .bind(post.score)
        .bind(post.fav_count)
        .bind(&post.file_ext)
        .bind(post.file_size.map(|size| size as i64))
        .bind(&post.file_url)
        .bind(&post.created_at)
        .bind(post.created_at.as_deref().and_then(timestamp::parse))
        .bind(&post.post_url)
        .bind(path.to_string_lossy().into_owned())
        .bind(downloaded_at)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM post_tags WHERE post_id = ?").bind(id).execute(&mut *tx).await?;
        for (category, names) in post.tags.by_category() {
            for name in names {
                sqlx::query(
                    "INSERT INTO tags (name, category) VALUES (?, ?)
                     ON CONFLICT (name) DO UPDATE SET category = excluded.category
                     WHERE tags.category = 'general' AND excluded.category <> 'general'",
                )
                .bind(name)
                .bind(category)
                .execute(&mut *tx)
                .await?;
                sqlx::query("INSERT OR IGNORE INTO post_tags (post_id, tag_id) SELECT ?, id FROM tags WHERE name = ?")
                    .bind(id)
                    .bind(name)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await
    }

    /// 把筛选条件里的 tag 名换成 id。要「有」的 tag 图库里根本没有时，结果一定为空，返回 `None`。
    async fn resolve_tags(&self, tags: &str) -> Result<Option<TagFilter>, sqlx::Error> {
        let mut filter = TagFilter::default();
        let mut seen = HashSet::new();
        for tag in tags.split_whitespace().filter(|tag| seen.insert(*tag)) {
            let (negate, name) = match tag.strip_prefix('-') {
                Some(name) if !name.is_empty() => (true, name),
                _ => (false, tag),
            };
            let id: Option<i64> = sqlx::query_scalar("SELECT id FROM tags WHERE name = ?")
                .bind(name.to_lowercase())
                .fetch_optional(&self.pool)
                .await?;
            match (id, negate) {
                (Some(id), false) => filter.include.push(id),
                (Some(id), true) => filter.exclude.push(id),
                (None, false) => return Ok(None),
                (None, true) => {}
            }
        }
        Ok(Some(filter))
    }

    pub async fn list(&self, query: &LibraryQuery) -> Result<LibraryPage, sqlx::Error> {
        let limit = match query.limit {
            0 => 60,
            n => n.min(MAX_PAGE),
        };
        let Some(tags) = self.resolve_tags(&query.tags).await? else {
            return Ok(LibraryPage { posts: Vec::new(), total: 0, offset: query.offset, has_more: false });
        };
        let mut count = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM posts p");
        push_filter(&mut count, query, &tags);
        let total: i64 = count.build_query_scalar().fetch_one(&self.pool).await?;

        // 先只按排序取出这一页的 id，再取整行：排序时不用搬动几万行完整记录，热门 tag 这类结果多的查询快几倍。
        let mut select = QueryBuilder::<Sqlite>::new("SELECT p.* FROM posts p WHERE p.id IN (SELECT p.id FROM posts p");
        push_filter(&mut select, query, &tags);
        select
            .push(" ORDER BY ")
            .push(query.sort.order_by())
            .push(" LIMIT ")
            .push_bind(limit as i64)
            .push(" OFFSET ")
            .push_bind(query.offset as i64)
            .push(") ORDER BY ")
            .push(query.sort.order_by());
        let rows = select.build().fetch_all(&self.pool).await?;
        let mut posts = Vec::with_capacity(rows.len());
        let mut row_ids = Vec::with_capacity(rows.len());
        for row in &rows {
            row_ids.push(row.try_get::<i64, _>("id")?);
            posts.push(post_from_row(row)?);
        }

        if !row_ids.is_empty() {
            let mut tags = QueryBuilder::<Sqlite>::new(
                "SELECT pt.post_id, t.name, t.category FROM post_tags pt JOIN tags t ON t.id = pt.tag_id WHERE pt.post_id IN (",
            );
            let mut list = tags.separated(", ");
            for id in &row_ids {
                list.push_bind(*id);
            }
            tags.push(") ORDER BY t.name");
            let mut grouped: HashMap<i64, PostTags> = HashMap::new();
            for row in tags.build().fetch_all(&self.pool).await? {
                let category: String = row.try_get("category")?;
                grouped.entry(row.try_get("post_id")?).or_default().push(&category, row.try_get("name")?);
            }
            for (post, id) in posts.iter_mut().zip(&row_ids) {
                if let Some(tags) = grouped.remove(id) {
                    post.post.tags = tags;
                }
            }
        }

        let has_more = (query.offset as i64 + posts.len() as i64) < total;
        Ok(LibraryPage { posts, total, offset: query.offset, has_more })
    }

    // ---------- 文件夹视图 ----------

    /// 按来源分的文件夹，每个带张数和最近下载的几张封面。没有图的来源也列出来。
    pub async fn folders(&self) -> Result<Vec<Folder>, sqlx::Error> {
        // 张数只数来源索引就够了；最近下载时间就是第一张封面的时间，不用再把整张表扫一遍。
        let counts: Vec<(String, i64)> =
            sqlx::query_as("SELECT source, COUNT(*) FROM posts GROUP BY source").fetch_all(&self.pool).await?;
        let mut folders = Vec::with_capacity(Source::ALL.len());
        for source in Source::ALL {
            let count = counts.iter().find(|(name, _)| name == source.as_str()).map_or(0, |(_, count)| *count);
            let rows: Vec<(i64, u32, u32, i64)> = if count == 0 {
                Vec::new()
            } else {
                sqlx::query_as(
                    "SELECT post_id, width, height, downloaded_at FROM posts WHERE source = ? \
                     ORDER BY downloaded_at DESC, id DESC LIMIT ?",
                )
                .bind(source.as_str())
                .bind(COVERS)
                .fetch_all(&self.pool)
                .await?
            };
            let latest_at = rows.first().map(|row| row.3);
            let covers = rows.into_iter().map(|(id, width, height, _)| cover(source, id, width, height)).collect();
            folders.push(Folder { source, count, latest_at, covers });
        }
        Ok(folders)
    }

    /// 文件夹里按画师、作品、角色或一般 tag 分组，每组带张数和最近下载的几张封面。
    /// 没有这类 tag 的图不在任何一组里，从「全部」里看。
    pub async fn groups(&self, query: &GroupQuery) -> Result<GroupPage, sqlx::Error> {
        let limit = match query.limit {
            0 => 60,
            n => n.min(MAX_PAGE),
        };
        // 用 CROSS JOIN 固定连接顺序。统计信息过时的连接上（例如软件开着时图库从空的涨到几万张），
        // SQLite 自己挑的顺序可能慢好几倍。画师、作品、角色从 tag 一侧查：每张图只有一两个这类 tag，
        // 要看的关联很少（十万张约 70 ms）；一般 tag 每张图有十几二十个，从图一侧顺着查更快（约 0.7 秒，从 tag 一侧要 1.3 秒）。
        let from = match query.kind {
            GroupKind::General => {
                "posts p CROSS JOIN post_tags pt ON pt.post_id = p.id CROSS JOIN tags t ON t.id = pt.tag_id"
            }
            _ => "tags t CROSS JOIN post_tags pt ON pt.tag_id = t.id CROSS JOIN posts p ON p.id = pt.post_id",
        };
        let sql = format!(
            "SELECT t.id, t.name, COUNT(*) AS count, MAX(p.downloaded_at) AS latest, COUNT(*) OVER () AS total
             FROM {from}
             WHERE t.category = ? AND p.source = ?
             GROUP BY t.id
             ORDER BY {}
             LIMIT ? OFFSET ?",
            query.sort.order_by()
        );
        // 拼进去的只有固定的连接顺序和排序子句，条件都走参数绑定。
        let rows: Vec<(i64, String, i64, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(query.kind.category())
            .bind(query.source.as_str())
            .bind(i64::from(limit))
            .bind(i64::from(query.offset))
            .fetch_all(&self.pool)
            .await?;
        // 翻过了最后一页时拿不到总数，按已经翻过的算。
        let total = rows.first().map_or(i64::from(query.offset), |row| row.4);

        // 封面也固定顺序：先按这一页的 tag 找关联，再查图。反过来会把整个来源的图都过一遍，慢十几到几十倍。
        let mut covers: HashMap<i64, Vec<Cover>> = HashMap::new();
        if !rows.is_empty() {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "SELECT tag_id, post_id, width, height FROM (
                   SELECT pt.tag_id, p.post_id, p.width, p.height,
                          ROW_NUMBER() OVER (PARTITION BY pt.tag_id ORDER BY p.downloaded_at DESC, p.id DESC) AS rn
                   FROM post_tags pt CROSS JOIN posts p ON p.id = pt.post_id
                   WHERE p.source = ",
            );
            sql.push_bind(query.source.as_str()).push(" AND pt.tag_id IN (");
            let mut ids = sql.separated(", ");
            for row in &rows {
                ids.push_bind(row.0);
            }
            sql.push(")) WHERE rn <= ").push_bind(COVERS).push(" ORDER BY tag_id, rn");
            let cover_rows: Vec<(i64, i64, u32, u32)> = sql.build_query_as().fetch_all(&self.pool).await?;
            for (tag_id, post_id, width, height) in cover_rows {
                covers.entry(tag_id).or_default().push(cover(query.source, post_id, width, height));
            }
        }

        let groups: Vec<Group> = rows
            .into_iter()
            .map(|(tag_id, name, count, latest_at, _)| Group {
                name,
                count,
                latest_at,
                covers: covers.remove(&tag_id).unwrap_or_default(),
            })
            .collect();
        let has_more = i64::from(query.offset) + (groups.len() as i64) < total;
        Ok(GroupPage { groups, total, has_more })
    }

    /// 从图库删除记录（tag 关联随之删除），返回删掉的条数。不碰文件。
    pub async fn remove_posts(&self, posts: &[(Source, u64)]) -> Result<u64, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let mut removed = 0;
        for (source, post_id) in posts {
            removed += sqlx::query("DELETE FROM posts WHERE source = ? AND post_id = ?")
                .bind(source.as_str())
                .bind(*post_id as i64)
                .execute(&mut *tx)
                .await?
                .rows_affected();
        }
        tx.commit().await?;
        Ok(removed)
    }

    /// 图片位置整体移走后，把旧位置下的路径改成新位置。返回改了多少条。
    pub async fn rebase_paths(&self, from: &Path, to: &Path) -> Result<u64, sqlx::Error> {
        let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, local_path FROM posts").fetch_all(&self.pool).await?;
        let mut tx = self.pool.begin().await?;
        let mut changed = 0;
        for (id, path) in rows {
            let Ok(rest) = Path::new(&path).strip_prefix(from) else { continue };
            sqlx::query("UPDATE posts SET local_path = ? WHERE id = ?")
                .bind(to.join(rest).to_string_lossy().into_owned())
                .bind(id)
                .execute(&mut *tx)
                .await?;
            changed += 1;
        }
        tx.commit().await?;
        Ok(changed)
    }

    // ---------- 订阅 ----------

    pub async fn create_subscription(&self, new: NewSubscription<'_>) -> Result<Subscription, sqlx::Error> {
        let now = now_ms();
        let ratings: Vec<&str> = new.ratings.iter().map(|r| r.as_str()).collect();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO subscriptions (source, tags, ratings, query, interval_minutes, last_seen_id, local_filter,
                                        last_checked_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(new.source.as_str())
        .bind(new.tags.trim())
        .bind(ratings.join(","))
        .bind(new.query)
        .bind(new.interval_minutes)
        .bind(new.last_seen_id)
        .bind(new.local_filter.filter(|f| !f.is_empty()))
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        self.subscription(id)
            .await?
            .ok_or_else(|| db_err(tr!("订阅写入后读取失败", "Couldn't read the subscription back after saving it")))
    }

    pub async fn subscription(&self, id: i64) -> Result<Option<Subscription>, sqlx::Error> {
        let sql = format!("SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions s WHERE s.id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(id).fetch_optional(&self.pool).await?;
        row.as_ref().map(subscription_from_row).transpose()
    }

    /// 全部订阅，新的在前。
    pub async fn subscriptions(&self) -> Result<Vec<Subscription>, sqlx::Error> {
        let sql = format!("SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions s ORDER BY s.id DESC");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_all(&self.pool).await?;
        rows.iter().map(subscription_from_row).collect()
    }

    // ---------- 收藏的搜索 ----------

    /// 收藏的搜索，后收藏的在前。
    pub async fn saved_searches(&self) -> Result<Vec<SavedSearch>, sqlx::Error> {
        let rows: Vec<(i64, String, String, String, String, i64)> =
            sqlx::query_as("SELECT id, source, tags, ratings, sort, created_at FROM saved_searches ORDER BY id DESC")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, source, tags, ratings, sort, created_at)| {
                let sources = split_sources(&source);
                (!sources.is_empty()).then(|| SavedSearch {
                    id,
                    sources,
                    tags,
                    ratings: ratings.split(',').filter_map(Rating::parse).collect(),
                    sort: Sort::parse(&sort).unwrap_or_default(),
                    created_at,
                })
            })
            .collect())
    }

    /// 收藏一个搜索条件；同样的条件已经收藏过时什么也不做。
    pub async fn add_saved_search(
        &self,
        sources: &[Source],
        tags: &str,
        ratings: &[Rating],
        sort: Sort,
    ) -> Result<(), sqlx::Error> {
        let (tags, ratings) = normalize_search(tags, ratings);
        sqlx::query(
            "INSERT OR IGNORE INTO saved_searches (source, tags, ratings, sort, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(join_sources(sources))
        .bind(tags)
        .bind(ratings)
        .bind(sort.as_str())
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_saved_search(&self, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM saved_searches WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// 到了检查时间的订阅。
    pub async fn due_subscriptions(&self, now: i64) -> Result<Vec<Subscription>, sqlx::Error> {
        let sql = format!(
            "SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions s
             WHERE s.enabled = 1 AND (s.last_checked_at IS NULL OR s.last_checked_at + s.interval_minutes * 60000 <= ?)
             ORDER BY s.last_checked_at"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(now).fetch_all(&self.pool).await?;
        rows.iter().map(subscription_from_row).collect()
    }

    pub async fn update_subscription(
        &self,
        id: i64,
        enabled: Option<bool>,
        interval_minutes: Option<i64>,
    ) -> Result<Option<Subscription>, sqlx::Error> {
        sqlx::query(
            "UPDATE subscriptions SET enabled = COALESCE(?, enabled), interval_minutes = COALESCE(?, interval_minutes),
                updated_at = ? WHERE id = ?",
        )
        .bind(enabled)
        .bind(interval_minutes)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;
        self.subscription(id).await
    }

    pub async fn delete_subscription(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM subscriptions WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    /// 开始一次检查：建一个从「已处理到的 id」往新的方向翻页的下载任务，并清零上次的结果。
    pub async fn start_subscription_check(&self, sub: &Subscription) -> Result<JobInfo, sqlx::Error> {
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        let job_id: i64 = sqlx::query_scalar(
            "INSERT INTO jobs (kind, source, title, query, status, cursor, subscription_id, local_filter,
                               created_at, updated_at)
             VALUES ('query', ?, ?, ?, 'queued', ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(sub.source.as_str())
        .bind({
            let title = sub.title();
            tr!("订阅：{title}", "Subscription: {title}")
        })
        .bind(&sub.query)
        .bind(format!("a{}", sub.last_seen_id))
        .bind(sub.id)
        .bind(&sub.local_filter)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE subscriptions SET last_checked_at = ?, last_new = 0, last_error = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(now)
        .bind(now)
        .bind(sub.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.job(job_id).await?.ok_or_else(job_missing)
    }

    /// 检查任务结束后记下出错原因（成功时清空）。
    pub async fn finish_subscription_check(&self, id: i64, error: Option<&str>) -> Result<Option<Subscription>, sqlx::Error> {
        sqlx::query("UPDATE subscriptions SET last_error = ?, updated_at = ? WHERE id = ?")
            .bind(error)
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.subscription(id).await
    }

    // ---------- 下载任务 ----------

    pub async fn create_posts_job(&self, source: Source, title: &str, posts: &[Post]) -> Result<JobInfo, sqlx::Error> {
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO jobs (kind, source, title, status, total, created_at, updated_at)
             VALUES ('posts', ?, ?, 'queued', ?, ?, ?) RETURNING id",
        )
        .bind(source.as_str())
        .bind(title)
        .bind(posts.len() as i64)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
        insert_items(&mut tx, id, 0, posts).await?;
        tx.commit().await?;
        self.job(id).await?.ok_or_else(job_missing)
    }

    pub async fn create_query_job(
        &self,
        source: Source,
        title: &str,
        query: &str,
        local_filter: Option<&str>,
        max_posts: Option<i64>,
        estimate: Option<i64>,
    ) -> Result<JobInfo, sqlx::Error> {
        let now = now_ms();
        let total = match (estimate, max_posts) {
            (Some(estimate), Some(max)) => Some(estimate.min(max)),
            (estimate, _) => estimate,
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO jobs (kind, source, title, query, local_filter, max_posts, status, total, cursor,
                               created_at, updated_at)
             VALUES ('query', ?, ?, ?, ?, ?, 'queued', ?, '1', ?, ?) RETURNING id",
        )
        .bind(source.as_str())
        .bind(title)
        .bind(query)
        .bind(local_filter.filter(|f| !f.is_empty()))
        .bind(max_posts)
        .bind(total)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        self.job(id).await?.ok_or_else(job_missing)
    }

    pub async fn job(&self, id: i64) -> Result<Option<JobInfo>, sqlx::Error> {
        let sql = format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(id).fetch_optional(&self.pool).await?;
        row.as_ref().map(job_from_row).transpose()
    }

    /// 全部任务，新的在前。
    pub async fn jobs(&self) -> Result<Vec<JobInfo>, sqlx::Error> {
        let sql = format!("SELECT {JOB_COLUMNS} FROM jobs ORDER BY id DESC");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_all(&self.pool).await?;
        rows.iter().map(job_from_row).collect()
    }

    /// 下一个要跑的任务：上次没跑完的优先，其余按加入顺序。
    pub async fn next_job(&self) -> Result<Option<JobInfo>, sqlx::Error> {
        let sql = format!(
            "SELECT {JOB_COLUMNS} FROM jobs WHERE status IN ('running', 'queued')
             ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END, id LIMIT 1"
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_optional(&self.pool).await?;
        row.as_ref().map(job_from_row).transpose()
    }

    /// 只有状态在 `from` 里时才改成 `to`，返回改后的任务；状态不符时返回 `None`。
    pub async fn transition(
        &self,
        id: i64,
        from: &[JobStatus],
        to: JobStatus,
        error: Option<&str>,
    ) -> Result<Option<JobInfo>, sqlx::Error> {
        let mut query = QueryBuilder::<Sqlite>::new("UPDATE jobs SET status = ");
        query
            .push_bind(to.as_str())
            .push(", error = ")
            .push_bind(error)
            .push(", updated_at = ")
            .push_bind(now_ms())
            .push(" WHERE id = ")
            .push_bind(id)
            .push(" AND status IN (");
        let mut list = query.separated(", ");
        for status in from {
            list.push_bind(status.as_str());
        }
        query.push(")");
        if query.build().execute(&self.pool).await?.rows_affected() == 0 {
            return Ok(None);
        }
        self.job(id).await
    }

    /// 启动时把上次没跑完的任务放回队列。
    pub async fn requeue_interrupted(&self) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE jobs SET status = 'queued' WHERE status = 'running'").execute(&self.pool).await?;
        Ok(())
    }

    /// 按条件下载时追加一页帖子，同时记下下一页；`cursor` 为 `None` 表示已翻完，此时把总数改成实际张数。
    /// 站点没给总数时，翻完之前总数保持未知。
    /// `seen_max` 是站点这一页里最大的帖子 id（包括被本地筛选掉的），订阅按它推进。
    pub async fn append_items(
        &self,
        job_id: i64,
        posts: &[Post],
        cursor: Option<String>,
        seen_max: Option<u64>,
    ) -> Result<JobInfo, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let next_seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq) + 1, 0) FROM job_items WHERE job_id = ?")
            .bind(job_id)
            .fetch_one(&mut *tx)
            .await?;
        insert_items(&mut tx, job_id, next_seq, posts).await?;
        // 订阅检查：记下处理到哪里、找到了几张新图。任务内容和进度在同一个事务里，中断后从这里继续。
        if let Some(max_id) = seen_max.or_else(|| posts.iter().map(|post| post.id).max()).map(|id| id as i64) {
            sqlx::query(
                "UPDATE subscriptions SET last_seen_id = MAX(last_seen_id, ?), last_new = last_new + ?, updated_at = ?
                 WHERE id = (SELECT subscription_id FROM jobs WHERE id = ?)",
            )
            .bind(max_id)
            .bind(posts.len() as i64)
            .bind(now_ms())
            .bind(job_id)
            .execute(&mut *tx)
            .await?;
        }
        let exhausted = cursor.is_none();
        sqlx::query(
            "UPDATE jobs SET cursor = ?, updated_at = ?,
                total = CASE WHEN ? THEN (SELECT COUNT(*) FROM job_items WHERE job_id = ?)
                             WHEN total IS NULL THEN NULL
                             ELSE MAX(total, (SELECT COUNT(*) FROM job_items WHERE job_id = ?)) END
             WHERE id = ?",
        )
        .bind(cursor)
        .bind(now_ms())
        .bind(exhausted)
        .bind(job_id)
        .bind(job_id)
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.job(job_id).await?.ok_or_else(|| db_err(tr!("任务已被删除", "The job has been removed")))
    }

    pub async fn item_count(&self, job_id: i64) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT COUNT(*) FROM job_items WHERE job_id = ?").bind(job_id).fetch_one(&self.pool).await
    }

    /// 序号大于 `after_seq` 的待下载项，按顺序取。
    pub async fn pending_items(&self, job_id: i64, after_seq: i64, limit: i64) -> Result<Vec<JobItem>, sqlx::Error> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT seq, data FROM job_items WHERE job_id = ? AND status = 'pending' AND seq > ? ORDER BY seq LIMIT ?",
        )
        .bind(job_id)
        .bind(after_seq)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(seq, data)| {
                let post = serde_json::from_str(&data)
                    .map_err(|e| db_err(tr!("任务数据无法读取：{e}", "Couldn't read the job data: {e}")))?;
                Ok(JobItem { seq, post })
            })
            .collect()
    }

    pub async fn pending_count(&self, job_id: i64) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT COUNT(*) FROM job_items WHERE job_id = ? AND status = 'pending'")
            .bind(job_id)
            .fetch_one(&self.pool)
            .await
    }

    /// 记下一张图的结果并累加任务计数，返回更新后的任务。
    pub async fn finish_item(
        &self,
        job_id: i64,
        seq: i64,
        status: ItemStatus,
        note: Option<&str>,
    ) -> Result<Option<JobInfo>, sqlx::Error> {
        let counter = match status {
            ItemStatus::Saved => "saved",
            ItemStatus::Skipped => "skipped",
            ItemStatus::Failed => "failed",
            ItemStatus::Pending => return self.job(job_id).await,
        };
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query("UPDATE job_items SET status = ?, note = ? WHERE job_id = ? AND seq = ? AND status = 'pending'")
            .bind(status.as_str())
            .bind(note)
            .bind(job_id)
            .bind(seq)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if changed > 0 {
            let sql = format!("UPDATE jobs SET {counter} = {counter} + 1, updated_at = ? WHERE id = ?");
            sqlx::query(sqlx::AssertSqlSafe(sql)).bind(now_ms()).bind(job_id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        self.job(job_id).await
    }

    /// 失败的项重新排队。任务没在运行时放回队列。
    pub async fn retry_failed(&self, job_id: i64) -> Result<Option<JobInfo>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE job_items SET status = 'pending', note = NULL WHERE job_id = ? AND status = 'failed'")
            .bind(job_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE jobs SET failed = 0, error = NULL, updated_at = ?,
                status = CASE WHEN status = 'running' THEN 'running' ELSE 'queued' END
             WHERE id = ?",
        )
        .bind(now_ms())
        .bind(job_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.job(job_id).await
    }

    /// 失败和跳过的项，供任务详情显示原因。
    pub async fn item_notes(&self, job_id: i64) -> Result<Vec<ItemNote>, sqlx::Error> {
        let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
            "SELECT post_id, status, note FROM job_items WHERE job_id = ? AND status IN ('failed', 'skipped') ORDER BY seq",
        )
        .bind(job_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(post_id, status, note)| ItemNote {
                post_id,
                status: ItemStatus::parse(&status).unwrap_or(ItemStatus::Failed).as_str(),
                note,
            })
            .collect())
    }

    pub async fn delete_job(&self, id: i64) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM jobs WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    /// 删掉已完成和已取消的任务，返回删掉的 id。
    pub async fn clear_finished(&self) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar("DELETE FROM jobs WHERE status IN ('done', 'canceled') RETURNING id").fetch_all(&self.pool).await
    }
}

async fn insert_items(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    job_id: i64,
    first_seq: i64,
    posts: &[Post],
) -> Result<(), sqlx::Error> {
    for (offset, post) in posts.iter().enumerate() {
        let data = serde_json::to_string(post).map_err(|e| db_err(e.to_string()))?;
        sqlx::query("INSERT INTO job_items (job_id, seq, post_id, data, status) VALUES (?, ?, ?, ?, 'pending')")
            .bind(job_id)
            .bind(first_seq + offset as i64)
            .bind(post.id as i64)
            .bind(data)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// 图库筛选里的 tag，已经换成了 id。
#[derive(Default)]
struct TagFilter {
    include: Vec<i64>,
    exclude: Vec<i64>,
}

/// tag 都从 tag 那一侧查（post_tags 按 tag_id 建了索引）：要「有」的冷门 tag 只碰到几条记录；
/// 排除的 tag 先取出带这个 tag 的帖子再排除，比逐个帖子查快几倍。
///
/// 不写「WHERE 1 = 1」凑条件：实测多了这个恒真条件，SQLite 会放弃按排序列的索引，整表取出再排序。
fn push_filter(query: &mut QueryBuilder<Sqlite>, filter: &LibraryQuery, tags: &TagFilter) {
    let mut first = true;
    let mut and = |query: &mut QueryBuilder<Sqlite>| {
        query.push(if std::mem::take(&mut first) { " WHERE " } else { " AND " });
    };
    if let Some(source) = filter.source {
        and(query);
        query.push("p.source = ").push_bind(source.as_str());
    }
    let ratings: Vec<Rating> = Rating::ALL.into_iter().filter(|r| filter.ratings.contains(r)).collect();
    if !ratings.is_empty() && ratings.len() < Rating::ALL.len() {
        and(query);
        query.push("p.rating IN (");
        let mut list = query.separated(", ");
        for rating in ratings {
            list.push_bind(rating.as_str());
        }
        query.push(")");
    }
    for id in &tags.include {
        and(query);
        query.push("p.id IN (SELECT post_id FROM post_tags WHERE tag_id = ").push_bind(*id).push(")");
    }
    for id in &tags.exclude {
        and(query);
        query.push("p.id NOT IN (SELECT post_id FROM post_tags WHERE tag_id = ").push_bind(*id).push(")");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(source: Source, id: u64, tags: PostTags) -> Post {
        Post {
            source,
            id,
            md5: Some(format!("{id:032x}")),
            width: 800,
            height: 600,
            rating: Some(Rating::General),
            score: 5,
            fav_count: Some(3),
            file_ext: "png".into(),
            file_size: Some(1234),
            file_url: Some(format!("https://cdn.donmai.us/original/{id}.png")),
            sample_url: None,
            thumb_url: None,
            created_at: Some("2026-09-27T00:00:00Z".into()),
            post_url: format!("https://danbooru.donmai.us/posts/{id}"),
            tags,
            pages: None,
        }
    }

    fn tags(artist: &[&str], general: &[&str]) -> PostTags {
        PostTags {
            artist: artist.iter().map(|s| s.to_string()).collect(),
            general: general.iter().map(|s| s.to_string()).collect(),
            ..PostTags::default()
        }
    }

    #[tokio::test]
    async fn saves_posts_and_filters_by_tags() {
        let lib = Library::in_memory().await;
        lib.save_post(&post(Source::Danbooru, 1, tags(&["alice"], &["sky", "scenery"])), Path::new("/i/1.png"), 10)
            .await
            .unwrap();
        lib.save_post(&post(Source::Danbooru, 2, tags(&[], &["sky"])), Path::new("/i/2.png"), 20).await.unwrap();

        let all = lib.list(&LibraryQuery::default()).await.unwrap();
        assert_eq!(all.total, 2);
        // 新下载的在前。
        assert_eq!(all.posts[0].post.id, 2);
        assert_eq!(all.posts[1].post.tags.artist, vec!["alice"]);
        assert_eq!(all.posts[1].post.thumb_url.as_deref(), Some("local/thumb/danbooru/1"));

        let query = |tags: &str| LibraryQuery { tags: tags.into(), ..LibraryQuery::default() };
        assert_eq!(lib.list(&query("sky scenery")).await.unwrap().total, 1);
        assert_eq!(lib.list(&query("sky -scenery")).await.unwrap().posts[0].post.id, 2);
        assert_eq!(lib.list(&query("SKY")).await.unwrap().total, 2);
        // 图库里没有的 tag：要「有」时结果为空，排除时等于没写。
        assert_eq!(lib.list(&query("sky nobody")).await.unwrap().total, 0);
        assert_eq!(lib.list(&query("sky -nobody")).await.unwrap().total, 2);

        let probe = |source, id, md5: String| Post { md5: Some(md5), ..post(source, id, PostTags::default()) };
        let probes = vec![
            post(Source::Danbooru, 1, PostTags::default()),
            probe(Source::Danbooru, 3, "ab".repeat(16)),
            // 另一个站点同样的 id 不算；同一张图（md5 相同，大小写不同）在另一个站点算。
            probe(Source::Gelbooru, 1, "cd".repeat(16)),
            probe(Source::Gelbooru, 9, format!("{:032X}", 2)),
        ];
        let owned = lib.owned(&probes).await.unwrap();
        assert_eq!(owned, HashSet::from([(Source::Danbooru, 1), (Source::Gelbooru, 9)]));
        assert!(lib.owned(&[]).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn sorts_by_each_column() {
        let lib = Library::in_memory().await;
        // (id, 下载时间, 发布时间, 分数, 收藏, 宽, 文件大小)
        let rows = [
            (1, 30, Some("2026-09-01T00:00:00Z"), 9, Some(1), 1000, Some(500)),
            (2, 10, Some("Sat Sep 26 00:00:00 +0000 2026"), 3, None, 3000, Some(900)),
            (3, 20, None, 6, Some(8), 2000, None),
        ];
        for (id, downloaded, created, score, favs, width, size) in rows {
            let mut p = post(Source::Danbooru, id, PostTags::default());
            p.created_at = created.map(str::to_string);
            (p.score, p.fav_count, p.width, p.file_size) = (score, favs, width, size);
            lib.save_post(&p, Path::new(&format!("/i/{id}.png")), downloaded).await.unwrap();
        }
        let ids = |sort| {
            let lib = &lib;
            async move {
                let page = lib.list(&LibraryQuery { sort, ..LibraryQuery::default() }).await.unwrap();
                page.posts.iter().map(|p| p.post.id).collect::<Vec<_>>()
            }
        };
        assert_eq!(ids(LibrarySort::Downloaded).await, [1, 3, 2]);
        assert_eq!(ids(LibrarySort::DownloadedAsc).await, [2, 3, 1]);
        // 两个站点的时间格式都能比较；没有发布时间的排最后。
        assert_eq!(ids(LibrarySort::Newest).await, [2, 1, 3]);
        assert_eq!(ids(LibrarySort::Oldest).await, [1, 2, 3]);
        assert_eq!(ids(LibrarySort::Score).await, [1, 3, 2]);
        assert_eq!(ids(LibrarySort::Favorites).await, [3, 1, 2]);
        assert_eq!(ids(LibrarySort::Resolution).await, [2, 3, 1]);
        assert_eq!(ids(LibrarySort::Filesize).await, [2, 1, 3]);
    }

    #[tokio::test]
    async fn backfills_posted_at_for_old_rows() {
        let lib = Library::in_memory().await;
        lib.save_post(&post(Source::Danbooru, 1, PostTags::default()), Path::new("/i/1.png"), 1).await.unwrap();
        sqlx::query("UPDATE posts SET posted_at = NULL").execute(&lib.pool).await.unwrap();
        lib.backfill_posted_at().await.unwrap();
        let posted: Option<i64> = sqlx::query_scalar("SELECT posted_at FROM posts").fetch_one(&lib.pool).await.unwrap();
        assert_eq!(posted, timestamp::parse("2026-09-27T00:00:00Z"));
    }

    #[tokio::test]
    async fn folders_and_groups_show_latest_downloads() {
        let lib = Library::in_memory().await;
        // 下载时间按 id 递增：id 越大越新。
        let saves = [
            (Source::Danbooru, 1, tags(&["alice"], &["sky"])),
            (Source::Danbooru, 2, tags(&["bob"], &["sky"])),
            (Source::Danbooru, 3, tags(&["alice"], &["sea"])),
            (Source::Danbooru, 4, tags(&["alice", "bob"], &["sky"])),
            (Source::Gelbooru, 5, tags(&[], &["sky"])),
        ];
        for (source, id, tags) in saves {
            let path = format!("/images/{id}.png");
            lib.save_post(&post(source, id, tags), Path::new(&path), id as i64 * 1000).await.unwrap();
        }

        let folders = lib.folders().await.unwrap();
        // 每个站点一个文件夹，没下载过的站点张数为 0。
        assert_eq!(
            folders.iter().map(|f| (f.source, f.count)).collect::<Vec<_>>(),
            [
                (Source::Danbooru, 4),
                (Source::Gelbooru, 1),
                (Source::Yandere, 0),
                (Source::Pixiv, 0),
                (Source::X, 0),
            ]
        );
        let ids: Vec<u64> = folders[0].covers.iter().map(|c| c.post_id).collect();
        assert_eq!(ids, [4, 3, 2, 1]);
        assert_eq!(folders[0].covers[0].thumb_url, "local/thumb/danbooru/4");
        assert_eq!(folders[1].latest_at, Some(5000));

        let query = |kind, sort| GroupQuery { source: Source::Danbooru, kind, sort, offset: 0, limit: 0 };
        let page = lib.groups(&query(GroupKind::Artist, GroupSort::Recent)).await.unwrap();
        assert_eq!(page.total, 2);
        assert!(!page.has_more);
        // alice 和 bob 最近都有 #4，同时间的按 tag 先后；封面从新到旧。
        let alice = page.groups.iter().find(|g| g.name == "alice").unwrap();
        assert_eq!((alice.count, alice.latest_at), (3, 4000));
        assert_eq!(alice.covers.iter().map(|c| c.post_id).collect::<Vec<_>>(), [4, 3, 1]);

        let by_count = lib.groups(&query(GroupKind::General, GroupSort::Count)).await.unwrap();
        assert_eq!(by_count.groups.iter().map(|g| (g.name.as_str(), g.count)).collect::<Vec<_>>(), [("sky", 3), ("sea", 1)]);
        let by_name = lib.groups(&query(GroupKind::Artist, GroupSort::Name)).await.unwrap();
        assert_eq!(by_name.groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["alice", "bob"]);

        let second = lib
            .groups(&GroupQuery { limit: 1, offset: 1, ..query(GroupKind::Artist, GroupSort::Name) })
            .await
            .unwrap();
        assert_eq!((second.groups.len(), second.total, second.has_more), (1, 2, false));
    }

    #[tokio::test]
    async fn saved_all_sites_becomes_site_list() {
        let lib = Library::in_memory().await;
        // 上一版把「全部平台」存成 all；其中一条和已有的收藏重复。
        for (source, tags) in [("all", "sky"), ("all", "cloud"), ("danbooru,gelbooru", "cloud")] {
            sqlx::query("INSERT INTO saved_searches (source, tags, ratings, sort, created_at) VALUES (?, ?, '', 'newest', 0)")
                .bind(source)
                .bind(tags)
                .execute(&lib.pool)
                .await
                .unwrap();
        }
        sqlx::raw_sql(include_str!("../migrations/0007_saved_search_sources.sql")).execute(&lib.pool).await.unwrap();
        let saved = lib.saved_searches().await.unwrap();
        assert_eq!(saved.len(), 2);
        assert!(saved.iter().all(|item| item.sources == [Source::Danbooru, Source::Gelbooru]));
    }

    #[tokio::test]
    async fn saved_searches_dedupe_and_remove() {
        let lib = Library::in_memory().await;
        let (d, g) = (Source::Danbooru, Source::Gelbooru);
        lib.add_saved_search(&[d], " sky  cloud ", &[Rating::Sensitive, Rating::General], Sort::Score).await.unwrap();
        // 空格、分级顺序不同的同一个条件只存一份；全选和都不选也算同一个。
        lib.add_saved_search(&[d], "sky cloud", &[Rating::General, Rating::Sensitive], Sort::Score).await.unwrap();
        lib.add_saved_search(&[g], "", &Rating::ALL, Sort::Newest).await.unwrap();
        lib.add_saved_search(&[g], "", &[], Sort::Newest).await.unwrap();
        // 几个站点一起搜的和单个站点的分开存；站点的先后不影响。
        lib.add_saved_search(&[g, d], "sky cloud", &[Rating::General, Rating::Sensitive], Sort::Score).await.unwrap();
        lib.add_saved_search(&[d, g], "sky cloud", &[Rating::General, Rating::Sensitive], Sort::Score).await.unwrap();
        let saved = lib.saved_searches().await.unwrap();
        assert_eq!(saved.len(), 3);
        assert_eq!(saved[0].sources, [d, g]);
        assert_eq!((saved[2].sources.as_slice(), saved[2].tags.as_str(), saved[2].sort), (&[d][..], "sky cloud", Sort::Score));
        assert_eq!(saved[2].ratings, vec![Rating::General, Rating::Sensitive]);
        assert!(saved[1].ratings.is_empty());
        lib.remove_saved_search(saved[1].id).await.unwrap();
        assert_eq!(lib.saved_searches().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn general_category_never_overrides_specific() {
        let lib = Library::in_memory().await;
        lib.save_post(&post(Source::Danbooru, 1, tags(&["alice"], &[])), Path::new("/i/1.png"), 1).await.unwrap();
        // Gelbooru 的 tag 全是「一般」，不能把 alice 改回一般。
        lib.save_post(&post(Source::Gelbooru, 9, tags(&[], &["alice"])), Path::new("/i/9.png"), 2).await.unwrap();
        let page = lib.list(&LibraryQuery::default()).await.unwrap();
        let gel = page.posts.iter().find(|p| p.post.source == Source::Gelbooru).unwrap();
        assert_eq!(gel.post.tags.artist, vec!["alice"]);
    }

    #[tokio::test]
    async fn removes_posts_with_their_tags() {
        let lib = Library::in_memory().await;
        lib.save_post(&post(Source::Danbooru, 1, tags(&["alice"], &["sky"])), Path::new("/i/1.png"), 1).await.unwrap();
        lib.save_post(&post(Source::Danbooru, 2, tags(&[], &["sky"])), Path::new("/i/2.png"), 2).await.unwrap();
        assert_eq!(lib.remove_posts(&[(Source::Danbooru, 1), (Source::Danbooru, 9)]).await.unwrap(), 1);
        assert!(lib.local_path(Source::Danbooru, 1).await.unwrap().is_none());
        let sky = lib.list(&LibraryQuery { tags: "sky".into(), ..LibraryQuery::default() }).await.unwrap();
        assert_eq!(sky.total, 1);
        let links: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM post_tags").fetch_one(&lib.pool).await.unwrap();
        assert_eq!(links, 1);
    }

    #[tokio::test]
    async fn rebases_paths_under_moved_root() {
        let lib = Library::in_memory().await;
        lib.save_post(&post(Source::Danbooru, 1, PostTags::default()), Path::new("/old/danbooru/1.png"), 1)
            .await
            .unwrap();
        lib.save_post(&post(Source::Danbooru, 2, PostTags::default()), Path::new("/elsewhere/2.png"), 1).await.unwrap();
        assert_eq!(lib.rebase_paths(Path::new("/old"), Path::new("/new")).await.unwrap(), 1);
        assert_eq!(lib.local_path(Source::Danbooru, 1).await.unwrap().unwrap(), PathBuf::from("/new/danbooru/1.png"));
        assert_eq!(lib.local_path(Source::Danbooru, 2).await.unwrap().unwrap(), PathBuf::from("/elsewhere/2.png"));
    }

    #[tokio::test]
    async fn job_items_progress_and_retry() {
        let lib = Library::in_memory().await;
        let posts = [post(Source::Danbooru, 1, PostTags::default()), post(Source::Danbooru, 2, PostTags::default())];
        let job = lib.create_posts_job(Source::Danbooru, "选中的 2 张", &posts).await.unwrap();
        assert_eq!((job.status, job.total), (JobStatus::Queued, Some(2)));

        let items = lib.pending_items(job.id, -1, 10).await.unwrap();
        assert_eq!(items.iter().map(|i| i.post.id).collect::<Vec<_>>(), vec![1, 2]);
        lib.finish_item(job.id, items[0].seq, ItemStatus::Saved, None).await.unwrap();
        let job = lib.finish_item(job.id, items[1].seq, ItemStatus::Failed, Some("超时")).await.unwrap().unwrap();
        assert_eq!((job.saved, job.failed), (1, 1));
        // 同一项不会被重复计数。
        let job = lib.finish_item(job.id, items[1].seq, ItemStatus::Failed, Some("超时")).await.unwrap().unwrap();
        assert_eq!(job.failed, 1);

        let job = lib.transition(job.id, &[JobStatus::Queued], JobStatus::Done, None).await.unwrap().unwrap();
        assert!(lib.transition(job.id, &[JobStatus::Queued], JobStatus::Paused, None).await.unwrap().is_none());
        let job = lib.retry_failed(job.id).await.unwrap().unwrap();
        assert_eq!((job.status, job.failed), (JobStatus::Queued, 0));
        assert_eq!(lib.pending_count(job.id).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn subscription_checks_advance_last_seen_id() {
        let lib = Library::in_memory().await;
        let sub = lib
            .create_subscription(NewSubscription {
                source: Source::Danbooru,
                tags: "sky ",
                ratings: &[Rating::General, Rating::Sensitive],
                query: "sky rating:g,s",
                interval_minutes: 60,
                last_seen_id: 100,
                local_filter: None,
            })
            .await
            .unwrap();
        assert_eq!((sub.tags.as_str(), sub.ratings.len(), sub.title()), ("sky", 2, "sky".to_string()));
        // 刚建好时已经算检查过一次，要等到下一个间隔。
        assert!(lib.due_subscriptions(now_ms()).await.unwrap().is_empty());
        assert_eq!(lib.due_subscriptions(now_ms() + 61 * 60_000).await.unwrap().len(), 1);

        let job = lib.start_subscription_check(&sub).await.unwrap();
        assert_eq!((job.cursor.as_deref(), job.subscription_id), (Some("a100"), Some(sub.id)));
        assert_eq!(lib.subscription(sub.id).await.unwrap().unwrap().active_job, Some(job.id));

        let page: Vec<Post> = [105, 101, 103].iter().map(|id| post(Source::Danbooru, *id, PostTags::default())).collect();
        lib.append_items(job.id, &page, Some("a105".into()), None).await.unwrap();
        let sub = lib.subscription(sub.id).await.unwrap().unwrap();
        assert_eq!((sub.last_seen_id, sub.last_new), (105, 3));

        lib.transition(job.id, &[JobStatus::Queued], JobStatus::Done, None).await.unwrap();
        assert_eq!(lib.subscription(sub.id).await.unwrap().unwrap().active_job, None);
        // 删掉订阅后任务留着，只断开关联。
        assert!(lib.delete_subscription(sub.id).await.unwrap());
        assert_eq!(lib.job(job.id).await.unwrap().unwrap().subscription_id, None);
    }

    #[tokio::test]
    async fn query_job_pages_update_total() {
        let lib = Library::in_memory().await;
        let job = lib.create_query_job(Source::Danbooru, "sky", "sky", None, Some(500), Some(900)).await.unwrap();
        assert_eq!((job.total, job.cursor.as_deref()), (Some(500), Some("1")));
        let page: Vec<Post> = (1..=3).map(|id| post(Source::Danbooru, id, PostTags::default())).collect();
        let job = lib.append_items(job.id, &page, Some("b1".into()), None).await.unwrap();
        assert_eq!((job.total, job.cursor.as_deref()), (Some(500), Some("b1")));
        let job = lib.append_items(job.id, &page[..1], None, None).await.unwrap();
        // 翻完后总数改成实际张数。
        assert_eq!((job.total, job.cursor), (Some(4), None));
        let seqs: Vec<i64> = lib.pending_items(job.id, -1, 10).await.unwrap().iter().map(|i| i.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2, 3]);

        // 站点没给总数：翻页期间保持未知，翻完才有。
        let job = lib.create_query_job(Source::Danbooru, "sky", "sky", Some("cloud"), None, None).await.unwrap();
        assert_eq!(job.local_filter.as_deref(), Some("cloud"));
        let job = lib.append_items(job.id, &page, Some("b1".into()), None).await.unwrap();
        assert_eq!(job.total, None);
        let job = lib.append_items(job.id, &[], None, None).await.unwrap();
        assert_eq!(job.total, Some(3));
    }
}
