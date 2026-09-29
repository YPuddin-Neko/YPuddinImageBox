//! 下载队列。
//!
//! - 一次跑一个任务，按加入顺序；上次没跑完的任务在下次启动后继续。
//! - 任务里最多 4 张同时下载（原图通道本身也限 4 个并发、每秒 5 次）。
//! - 按条件下载时边翻页边下载，下一页的页码存在任务里，重启后从断点继续。
//! - 边写临时文件边算 md5，大小和 md5 都对得上才改名成正式文件，再写进图库。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock, RwLockReadGuard};
use std::time::Duration;

use md5::{Digest, Md5};
use reqwest::header::REFERER;
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Notify};
use tokio::task::JoinSet;
use url::Url;

use crate::error::AppError;
use crate::i18n::{text, tr};
use crate::library::{now_ms, ItemStatus, JobInfo, JobItem, JobKind, JobStatus, Library, Subscription};
use crate::net::Net;
use crate::protocol::sniff;
use crate::sources::filter::LocalFilter;
use crate::sources::{self, kemono, pixiv, AccountStore, Page, Post, Source};
use crate::storage::{Storage, StorageKind};
use crate::thumbs;

const CONCURRENCY: usize = 4;
/// 每张图最多尝试几次（网络错误、服务器 5xx、校验不通过时重试）。
const ATTEMPTS: u32 = 3;
/// 原图可能有几十 MB，单个请求的超时放宽到 10 分钟。
const FILE_TIMEOUT: Duration = Duration::from_secs(600);
const IMAGE_EXTS: [&str; 6] = ["jpg", "jpeg", "png", "gif", "webp", "avif"];

/// 跳过或失败的原因，写进任务记录，按写入时的界面语言。
fn note_owned() -> &'static str {
    text("已在图库中", "Already in the library")
}

fn note_duplicate() -> &'static str {
    text("图库里已有同一张图", "The same image is already in the library")
}

fn note_not_image() -> &'static str {
    text(
        "不是图片（视频、动图压缩包等暂不下载）",
        "Not an image (videos, animation archives and the like aren't downloaded)",
    )
}

fn note_no_file(post: &Post) -> &'static str {
    if post.gold_only() {
        text("原图只对 Gold 及以上等级的账号开放", "The original is only available to Gold accounts and above")
    } else {
        text(
            "站点没有开放这张图的原图（画师被封禁或图片已下架时会这样）",
            "The site doesn't offer this original (this happens when the artist is banned or the image was taken down)",
        )
    }
}

fn note_bad_url() -> &'static str {
    text("原图地址无效", "Invalid original file address")
}

/// 推给界面的变化。
pub enum Event {
    Job(JobInfo),
    JobRemoved(i64),
    Saved { source: Source, post_id: u64 },
    Subscription(Subscription),
    /// 订阅检查下载到了新图，用系统通知告诉用户。
    NewPosts { title: String, saved: i64 },
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Pause,
    Cancel,
    Remove,
}

struct Active {
    job_id: i64,
    stop: watch::Sender<Option<Stop>>,
}

enum Outcome {
    Saved,
    Skipped(&'static str),
    Failed(String),
}

impl Outcome {
    fn status(&self) -> ItemStatus {
        match self {
            Outcome::Saved => ItemStatus::Saved,
            Outcome::Skipped(_) => ItemStatus::Skipped,
            Outcome::Failed(_) => ItemStatus::Failed,
        }
    }

    fn note(&self) -> Option<&str> {
        match self {
            Outcome::Saved => None,
            Outcome::Skipped(note) => Some(note),
            Outcome::Failed(note) => Some(note),
        }
    }
}

pub struct Downloader {
    library: Library,
    net: Arc<Net>,
    accounts: Arc<AccountStore>,
    storage: Arc<RwLock<Storage>>,
    /// 图片位置整体移动时拿写锁；每张图从选定目录到写进图库期间拿读锁，
    /// 保证新图不会写进正在搬走的目录。
    images_gate: Arc<tokio::sync::RwLock<()>>,
    events: EventSink,
    wake: Notify,
    /// 订阅有变化（新建、改间隔、启用）时叫醒调度，不必等满一分钟。
    schedule_wake: Notify,
    active: Mutex<Option<Active>>,
}

fn read(storage: &RwLock<Storage>) -> RwLockReadGuard<'_, Storage> {
    storage.read().unwrap_or_else(PoisonError::into_inner)
}

impl Downloader {
    pub fn new(
        library: Library,
        net: Arc<Net>,
        accounts: Arc<AccountStore>,
        storage: Arc<RwLock<Storage>>,
        images_gate: Arc<tokio::sync::RwLock<()>>,
        events: EventSink,
    ) -> Arc<Self> {
        Arc::new(Self {
            library,
            net,
            accounts,
            storage,
            images_gate,
            events,
            wake: Notify::new(),
            schedule_wake: Notify::new(),
            active: Mutex::new(None),
        })
    }

    fn emit(&self, event: Event) {
        (self.events)(event);
    }

    fn set_active(&self, active: Option<Active>) {
        *self.active.lock().unwrap_or_else(PoisonError::into_inner) = active;
    }

    /// 任务正在运行时通知它停下，返回 `true`；否则返回 `false`，由调用方直接改数据库。
    fn signal(&self, id: i64, stop: Stop) -> bool {
        let active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        match active.as_ref() {
            Some(active) if active.job_id == id => {
                active.stop.send_replace(Some(stop));
                true
            }
            _ => false,
        }
    }

    // ---------- 界面操作 ----------

    pub async fn enqueue_posts(&self, posts: Vec<Post>) -> Result<JobInfo, AppError> {
        let source = posts
            .first()
            .map(|p| p.source)
            .ok_or_else(|| AppError::Internal(tr!("没有选中图片", "No images selected")))?;
        if posts.iter().any(|p| p.source != source) {
            return Err(AppError::Internal(tr!(
                "一次只能下载同一个站点的图片",
                "Images from different sites can't be downloaded together"
            )));
        }
        let count = posts.len();
        let title = match posts.as_slice() {
            [post] => post.label(),
            _ => tr!("选中的 {count} 张", "{count} selected images"),
        };
        let job = self.library.create_posts_job(source, &title, &posts).await?;
        self.emit(Event::Job(job.clone()));
        self.wake.notify_one();
        Ok(job)
    }

    /// 按条件下载全部结果。`query` 发给站点，`local_filter` 是超出 tag 上限、在本地筛选的部分；
    /// `count_query` 是完整条件，用来估计总数（Danbooru 的计数接口不限 tag 数量）。
    pub async fn enqueue_query(
        &self,
        source: Source,
        title: &str,
        query: &str,
        local_filter: Option<&str>,
        count_query: &str,
        max_posts: Option<i64>,
    ) -> Result<JobInfo, AppError> {
        // 总数只用来显示进度，查不到也照样开始；缺账号则直接提示，不建一个注定失败的任务。
        let estimate = match sources::count(&self.net, &self.accounts.get(), source, count_query).await {
            Ok(count) => count.map(|n| n as i64),
            Err(err @ AppError::CredentialsMissing(_)) => return Err(err),
            Err(_) => None,
        };
        let job = self.library.create_query_job(source, title, query, local_filter, max_posts, estimate).await?;
        self.emit(Event::Job(job.clone()));
        self.wake.notify_one();
        Ok(job)
    }

    pub async fn pause(&self, id: i64) -> Result<(), AppError> {
        if !self.signal(id, Stop::Pause) {
            if let Some(job) = self.library.transition(id, &[JobStatus::Queued], JobStatus::Paused, None).await? {
                self.emit(Event::Job(job));
            }
        }
        Ok(())
    }

    pub async fn resume(&self, id: i64) -> Result<(), AppError> {
        let from = [JobStatus::Paused, JobStatus::Failed, JobStatus::Canceled];
        if let Some(job) = self.library.transition(id, &from, JobStatus::Queued, None).await? {
            self.emit(Event::Job(job));
            self.wake.notify_one();
        }
        Ok(())
    }

    pub async fn cancel(&self, id: i64) -> Result<(), AppError> {
        if !self.signal(id, Stop::Cancel) {
            let from = [JobStatus::Queued, JobStatus::Paused, JobStatus::Failed];
            if let Some(job) = self.library.transition(id, &from, JobStatus::Canceled, None).await? {
                self.emit(Event::Job(job));
            }
        }
        Ok(())
    }

    /// 失败的图重新下载。
    pub async fn retry(&self, id: i64) -> Result<(), AppError> {
        if let Some(job) = self.library.retry_failed(id).await? {
            self.emit(Event::Job(job));
            self.wake.notify_one();
        }
        Ok(())
    }

    pub async fn remove(&self, id: i64) -> Result<(), AppError> {
        if !self.signal(id, Stop::Remove) && self.library.delete_job(id).await? {
            self.emit(Event::JobRemoved(id));
        }
        Ok(())
    }

    pub async fn clear_finished(&self) -> Result<(), AppError> {
        for id in self.library.clear_finished().await? {
            self.emit(Event::JobRemoved(id));
        }
        Ok(())
    }

    // ---------- 订阅 ----------

    /// 立即检查一个订阅：建一个从上次处理到的 id 往新的方向翻页的下载任务。
    /// 这个订阅已经有检查任务在排队、下载或暂停时不重复建，返回 `None`。
    pub async fn check_subscription(&self, id: i64) -> Result<Option<JobInfo>, AppError> {
        let sub = self
            .library
            .subscription(id)
            .await?
            .ok_or_else(|| AppError::Internal(tr!("订阅不存在", "This subscription no longer exists")))?;
        if sub.active_job.is_some() {
            return Ok(None);
        }
        let job = self.library.start_subscription_check(&sub).await?;
        self.emit(Event::Job(job.clone()));
        if let Some(sub) = self.library.subscription(id).await? {
            self.emit(Event::Subscription(sub));
        }
        self.wake.notify_one();
        Ok(Some(job))
    }

    /// 立即检查全部启用的订阅，返回新建了几个检查任务。
    pub async fn check_all_subscriptions(&self) -> Result<usize, AppError> {
        let mut started = 0;
        for sub in self.library.subscriptions().await?.into_iter().filter(|sub| sub.enabled) {
            if self.check_subscription(sub.id).await?.is_some() {
                started += 1;
            }
        }
        Ok(started)
    }

    /// 订阅有变化时调用，让调度马上重新看一遍。
    pub fn reschedule(&self) {
        self.schedule_wake.notify_one();
    }

    /// 订阅调度，常驻后台：每分钟看一次哪些订阅到了检查时间。
    pub async fn run_schedule(self: Arc<Self>) {
        loop {
            if let Ok(due) = self.library.due_subscriptions(now_ms()).await {
                for sub in due.into_iter().filter(|sub| sub.active_job.is_none()) {
                    let _ = self.check_subscription(sub.id).await;
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                _ = self.schedule_wake.notified() => {}
            }
        }
    }

    /// 订阅检查任务结束：记下结果；没找到新图的任务直接删掉，免得下载列表里堆满空任务。
    async fn finish_subscription_job(&self, sub_id: i64, job: &JobInfo) {
        let error = (job.status == JobStatus::Failed).then(|| job.error.clone()).flatten();
        if let Ok(Some(sub)) = self.library.finish_subscription_check(sub_id, error.as_deref()).await {
            if job.status == JobStatus::Done && job.saved > 0 {
                self.emit(Event::NewPosts { title: sub.title(), saved: job.saved });
            }
            self.emit(Event::Subscription(sub));
        }
        if job.status == JobStatus::Done && job.total == Some(0) {
            if let Ok(true) = self.library.delete_job(job.id).await {
                self.emit(Event::JobRemoved(job.id));
            }
        }
    }

    // ---------- 队列 ----------

    /// 队列主循环，常驻后台。
    pub async fn run(self: Arc<Self>) {
        loop {
            match self.library.next_job().await {
                Ok(Some(job)) => self.run_job(job).await,
                Ok(None) => self.wake.notified().await,
                Err(err) => {
                    log::error!("读取下载任务失败：{err}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn run_job(self: &Arc<Self>, job: JobInfo) {
        let id = job.id;
        let (stop, stop_rx) = watch::channel(None);
        // 先登记再认领：界面在这之间点暂停，要么改数据库让认领失败，要么收到停止信号。
        self.set_active(Some(Active { job_id: id, stop }));
        let claimed =
            self.library.transition(id, &[JobStatus::Queued, JobStatus::Running], JobStatus::Running, None).await;
        let result = match claimed {
            Ok(Some(job)) => {
                log::info!("开始下载任务 #{id}「{}」", job.title);
                self.emit(Event::Job(job.clone()));
                self.process(job, stop_rx).await
            }
            Ok(None) => {
                self.set_active(None);
                return;
            }
            Err(err) => Err(err.into()),
        };
        self.set_active(None);

        let running = [JobStatus::Running];
        let finished = match result {
            // 跑完的一刻可能刚好有失败项被重新排队，这时放回队列再跑一轮。
            Ok(None) => match self.library.pending_count(id).await {
                Ok(pending) if pending > 0 => self.library.transition(id, &running, JobStatus::Queued, None).await,
                _ => self.library.transition(id, &running, JobStatus::Done, None).await,
            },
            Ok(Some(Stop::Pause)) => self.library.transition(id, &running, JobStatus::Paused, None).await,
            Ok(Some(Stop::Cancel)) => self.library.transition(id, &running, JobStatus::Canceled, None).await,
            Ok(Some(Stop::Remove)) => {
                if let Ok(true) = self.library.delete_job(id).await {
                    self.emit(Event::JobRemoved(id));
                }
                return;
            }
            Err(err) => {
                let message = err.to_string();
                self.library.transition(id, &running, JobStatus::Failed, Some(&message)).await
            }
        };
        if let Ok(Some(job)) = finished {
            log::info!(
                "下载任务 #{id} {}：保存 {}，跳过 {}，失败 {}{}",
                job.status.as_str(),
                job.saved,
                job.skipped,
                job.failed,
                job.error.as_deref().map(|e| format!("，原因：{e}")).unwrap_or_default()
            );
            self.emit(Event::Job(job.clone()));
            if let Some(sub_id) = job.subscription_id {
                if matches!(job.status, JobStatus::Done | JobStatus::Failed) {
                    self.finish_subscription_job(sub_id, &job).await;
                }
            }
        }
    }

    /// 跑一个任务，全部处理完返回 `None`，被叫停时返回原因。
    async fn process(
        self: &Arc<Self>,
        mut job: JobInfo,
        mut stop: watch::Receiver<Option<Stop>>,
    ) -> Result<Option<Stop>, AppError> {
        let mut queue: VecDeque<JobItem> = VecDeque::new();
        let mut last_seq = -1;
        let mut tasks: JoinSet<Outcome> = JoinSet::new();
        let mut running: HashMap<tokio::task::Id, (i64, Source, u64)> = HashMap::new();
        loop {
            let signal = *stop.borrow_and_update();
            if let Some(reason) = signal {
                // 进行中的下载直接中断，临时文件随之删除，这几张下次继续时重下。
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                return Ok(Some(reason));
            }

            if queue.is_empty() {
                let items = self.library.pending_items(job.id, last_seq, 32).await?;
                if let Some(last) = items.last() {
                    last_seq = last.seq;
                }
                queue.extend(items);
            }

            if queue.is_empty() && job.kind == JobKind::Query {
                if let Some(cursor) = job.cursor.clone() {
                    let fetched = tokio::select! {
                        biased;
                        _ = stop.changed() => continue,
                        fetched = self.fetch_page(&job, &cursor) => fetched?,
                    };
                    job = fetched;
                    self.emit(Event::Job(job.clone()));
                    continue;
                }
            }

            while tasks.len() < CONCURRENCY {
                let Some(item) = queue.pop_front() else { break };
                let key = (item.seq, item.post.source, item.post.id);
                let this = Arc::clone(self);
                let handle = tasks.spawn(async move { this.download(item.post).await });
                running.insert(handle.id(), key);
            }

            if tasks.is_empty() {
                // 运行中有失败项被重新排队（序号比已读到的小），从头再找一遍。
                if last_seq >= 0 && self.library.pending_count(job.id).await? > 0 {
                    last_seq = -1;
                    continue;
                }
                return Ok(None);
            }

            tokio::select! {
                _ = stop.changed() => {}
                joined = tasks.join_next_with_id() => {
                    let Some(joined) = joined else { continue };
                    let (task, outcome) = match joined {
                        Ok(done) => done,
                        Err(err) => {
                            (err.id(), Outcome::Failed(tr!("下载意外中断：{err}", "The download stopped unexpectedly: {err}")))
                        }
                    };
                    let Some((seq, source, post_id)) = running.remove(&task) else { continue };
                    if let Some(updated) =
                        self.library.finish_item(job.id, seq, outcome.status(), outcome.note()).await?
                    {
                        self.emit(Event::Job(updated));
                    }
                    if matches!(outcome, Outcome::Saved) {
                        self.emit(Event::Saved { source, post_id });
                    }
                }
            }
        }
    }

    /// 按条件下载：取下一页并追加到任务里，返回更新后的任务。
    async fn fetch_page(&self, job: &JobInfo, cursor: &str) -> Result<JobInfo, AppError> {
        let query = job.query.as_deref().unwrap_or_default();
        let page = Page::parse(cursor).unwrap_or(Page::Number(1));
        let limit = job.source.max_page_size();
        let accounts = self.accounts.get();
        let (mut posts, fetched) = sources::fetch(&self.net, &accounts, job.source, query, &page, limit).await?;
        let ids = posts.iter().map(|post| post.id);
        let bounds = ids.clone().min().zip(ids.max());
        // 未登录时站点会从结果里隐去部分帖子，一页不满不代表翻完了，取到空页才算。
        // 按 id 翻页时要靠这一页的 id 定下一页，一张都没留下就只能停；按页码翻的接着翻下一页。
        let mut exhausted = fetched == 0 || (bounds.is_none() && !matches!(page, Page::Number(_)));
        // 翻页位置按站点返回的整页算，本地筛选只决定哪些帖子进任务。
        if let Some(filter) = job.local_filter.as_deref().map(LocalFilter::parse) {
            posts.retain(|post| filter.matches(post));
        }
        if let Some(max) = job.max_posts {
            let room = (max - self.library.item_count(job.id).await?).max(0) as usize;
            if posts.len() >= room {
                posts.truncate(room);
                exhausted = true;
            }
        }
        let next = (!exhausted).then(|| page.next(job.source, query, bounds).to_param());
        Ok(self.library.append_items(job.id, &posts, next, bounds.map(|(_, max)| max)).await?)
    }

    // ---------- 单张图 ----------

    async fn download(&self, post: Post) -> Outcome {
        self.save(&post).await.unwrap_or_else(|err| Outcome::Failed(err.to_string()))
    }

    async fn save(&self, post: &Post) -> Result<Outcome, AppError> {
        if post.source == Source::Pixiv {
            return self.save_pixiv(post).await;
        }
        self.save_file(post).await
    }

    /// Pixiv 的作品下载时才取每一页的原图，每页在图库里各存一条。有一页失败就算这个作品失败，
    /// 重试时已存好的页直接跳过，只补失败的页。
    async fn save_pixiv(&self, work: &Post) -> Result<Outcome, AppError> {
        if pixiv::is_animation(work) {
            return Ok(Outcome::Skipped(note_not_image()));
        }
        let pages = match pixiv::pages(&self.net, self.accounts.get().pixiv.as_ref(), work).await {
            Ok(pages) => pages,
            Err(err) => return Ok(Outcome::Failed(err.to_string())),
        };
        let (mut saved, mut failed) = (false, None);
        for (index, page) in pages.iter().enumerate() {
            match self.save_file(page).await? {
                Outcome::Saved => saved = true,
                Outcome::Failed(err) => {
                    let page = index + 1;
                    failed.get_or_insert(tr!("第 {page} 页：{err}", "Page {page}: {err}"));
                }
                Outcome::Skipped(_) => {}
            }
        }
        Ok(match (saved, failed) {
            (saved, Some(err)) => {
                // 存上的页已经进了图库，照样通知界面刷新。
                if saved {
                    self.emit(Event::Saved { source: work.source, post_id: work.id });
                }
                Outcome::Failed(err)
            }
            (true, None) => Outcome::Saved,
            (false, None) => Outcome::Skipped(note_owned()),
        })
    }

    /// 下载一个文件（其他站点的一个帖子，或 Pixiv 作品的一页）。
    async fn save_file(&self, post: &Post) -> Result<Outcome, AppError> {
        if let Some(path) = self.library.local_path(post.source, post.id).await? {
            if exists(&path).await {
                return Ok(Outcome::Skipped(note_owned()));
            }
        }
        let Some(ext) = image_ext(post) else { return Ok(Outcome::Skipped(note_not_image())) };
        let Some(url) = post.file_url.as_deref() else { return Ok(Outcome::Failed(note_no_file(post).into())) };
        let url = match Url::parse(url) {
            // 只从帖子所属站点的域名下载。
            Ok(url) if sources::source_for_url(&url) == Some(post.source) => url,
            _ => return Ok(Outcome::Failed(note_bad_url().into())),
        };
        if let Some(md5) = post.md5.as_deref() {
            for path in self.library.paths_with_md5(md5, post.source, post.id).await? {
                if exists(&path).await {
                    return Ok(Outcome::Skipped(note_duplicate()));
                }
            }
        }

        let gate = self.images_gate.read().await;
        let root = read(&self.storage).path(StorageKind::Images);
        let target = target_path(&root, post, &ext);
        let mut attempt = 1;
        loop {
            match fetch_file(&self.net, &url, post, &target).await {
                Ok(()) => break,
                Err(err) if err.retryable() && attempt < ATTEMPTS => {
                    tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
                    attempt += 1;
                }
                Err(err) => return Ok(Outcome::Failed(err.to_string())),
            }
        }
        // 站点没给尺寸（Kemono、部分 X 图片记为 1 × 1）：按下载好的文件补上，图库里的比例才对。
        let sized = if post.width <= 1 || post.height <= 1 {
            image_size(target.clone()).await.map(|(width, height)| Post { width, height, ..post.clone() })
        } else {
            None
        };
        self.library.save_post(sized.as_ref().unwrap_or(post), &target, now_ms()).await?;
        drop(gate);

        // 缩略图生成失败不影响下载结果，浏览图库时会再生成。
        let cache = read(&self.storage).path(StorageKind::Cache);
        let _ = thumbs::generate(target, thumbs::path(&cache, post.source, post.id)).await;
        Ok(Outcome::Saved)
    }
}

/// 只读文件头得到宽高，不解码整张图。
async fn image_size(path: PathBuf) -> Option<(u32, u32)> {
    tokio::task::spawn_blocking(move || image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_dimensions().ok())
        .await
        .ok()
        .flatten()
}

async fn exists(path: &Path) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(false)
}

/// 帖子的扩展名；不是图片时返回 `None`。
fn image_ext(post: &Post) -> Option<String> {
    let from_url = || {
        let url = Url::parse(post.file_url.as_deref()?).ok()?;
        let (_, ext) = url.path().rsplit_once('.')?;
        Some(ext.to_string())
    };
    let ext = Some(post.file_ext.clone()).filter(|ext| !ext.is_empty()).or_else(from_url)?.to_ascii_lowercase();
    IMAGE_EXTS.contains(&ext.as_str()).then_some(ext)
}

/// 保存位置：`图片位置/站点/画师/帖子id.扩展名`；没有画师 tag 时直接放在站点目录下。
/// Pixiv 的文件名和原图一样是「作品id_p页码」，Kemono 是「帖子id_p第几张」。
pub fn target_path(root: &Path, post: &Post, ext: &str) -> PathBuf {
    let mut dir = root.join(post.source.site_name());
    if let Some(artist) = post.tags.artist.first() {
        dir.push(safe_name(artist));
    }
    let name = match post.source {
        Source::Pixiv => {
            let (illust, page) = pixiv::split_id(post.id);
            format!("{illust}_p{page}")
        }
        Source::Kemono => {
            let (id, index) = kemono::split_id(post.id);
            format!("{id}_p{index}")
        }
        _ => post.id.to_string(),
    };
    dir.join(format!("{name}.{ext}"))
}

/// 把 tag 变成 Windows 和 macOS 都能用的文件夹名。
pub(crate) fn safe_name(name: &str) -> String {
    const MAX_CHARS: usize = 100;
    let trim = |s: &str| s.trim_start().trim_end_matches(['.', ' ']).to_string();
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    let mut cleaned = trim(&cleaned);
    if cleaned.chars().count() > MAX_CHARS {
        cleaned = trim(&cleaned.chars().take(MAX_CHARS).collect::<String>());
    }
    // 以点开头在 macOS 上是隐藏文件夹。
    if cleaned.starts_with('.') {
        cleaned.replace_range(..1, "_");
    }
    if cleaned.is_empty() {
        return "_".into();
    }
    let stem = cleaned.split('.').next().unwrap_or_default().to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.len() == 4 && stem.starts_with(prefix) && stem.as_bytes()[3].is_ascii_digit()
    };
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT") {
        cleaned.insert(0, '_');
    }
    cleaned
}

#[derive(Debug)]
enum FetchError {
    Network(reqwest::Error),
    Status(u16),
    Incomplete,
    Checksum,
    NotImage,
    Io(std::io::Error),
    Other(String),
}

impl FetchError {
    fn retryable(&self) -> bool {
        match self {
            FetchError::Network(_) | FetchError::Incomplete | FetchError::Checksum => true,
            FetchError::Status(code) => *code == 408 || *code == 429 || *code >= 500,
            FetchError::NotImage | FetchError::Io(_) | FetchError::Other(_) => false,
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Network(err) => {
                let detail = crate::net::network_detail(err);
                f.write_str(&tr!("网络错误：{detail}", "Network error: {detail}"))
            }
            FetchError::Status(code) => f.write_str(&tr!("服务器返回 HTTP {code}", "The server returned HTTP {code}")),
            FetchError::Incomplete => f.write_str(text("文件没有下载完整", "The file didn't download completely")),
            FetchError::Checksum => {
                f.write_str(text("文件校验不通过（md5 不一致）", "Checksum mismatch (the md5 doesn't match)"))
            }
            FetchError::NotImage => f.write_str(text("下载到的不是图片", "The downloaded file isn't an image")),
            FetchError::Io(err) => f.write_str(&tr!("保存文件失败：{err}", "Couldn't save the file: {err}")),
            FetchError::Other(message) => f.write_str(message),
        }
    }
}

/// 下载中的临时文件；没下完（出错、任务被暂停或取消）时自动删掉。
struct PartFile {
    path: PathBuf,
    keep: bool,
}

impl PartFile {
    fn for_target(target: &Path) -> Self {
        let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self { path: target.with_file_name(format!(".{name}.part")), keep: false }
    }

    fn keep(mut self) {
        self.keep = true;
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

async fn fetch_file(net: &Net, url: &Url, post: &Post, target: &Path) -> Result<(), FetchError> {
    let request = net.client().get(url.clone()).header(REFERER, post.source.referer()).timeout(FILE_TIMEOUT);
    let mut response = net.file.send(request).await.map_err(|err| match err {
        AppError::Network(err) => FetchError::Network(err),
        other => FetchError::Other(other.to_string()),
    })?;
    if !response.status().is_success() {
        return Err(FetchError::Status(response.status().as_u16()));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(FetchError::Io)?;
    }
    let part = PartFile::for_target(target);
    let mut file = tokio::fs::File::create(&part.path).await.map_err(FetchError::Io)?;
    let mut hasher = Md5::new();
    let mut head = Vec::with_capacity(16);
    let mut size = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(FetchError::Network)? {
        if head.len() < 16 {
            head.extend_from_slice(&chunk[..chunk.len().min(16 - head.len())]);
        }
        hasher.update(&chunk);
        size += chunk.len() as u64;
        file.write_all(&chunk).await.map_err(FetchError::Io)?;
    }
    file.flush().await.map_err(FetchError::Io)?;
    drop(file);

    if sniff(&head).is_none() {
        return Err(FetchError::NotImage);
    }
    if post.file_size.is_some_and(|expected| expected != size) {
        return Err(FetchError::Incomplete);
    }
    if let Some(expected) = post.md5.as_deref() {
        let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(FetchError::Checksum);
        }
    }
    tokio::fs::rename(&part.path, target).await.map_err(FetchError::Io)?;
    part.keep();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ProxySettings;
    use crate::sources::{PostTags, Rating};
    use crate::storage::Defaults;

    fn post(id: u64, ext: &str, file_url: Option<&str>) -> Post {
        Post {
            source: Source::Danbooru,
            id,
            md5: None,
            width: 10,
            height: 10,
            rating: Some(Rating::General),
            score: 0,
            fav_count: None,
            file_ext: ext.into(),
            file_size: None,
            file_url: file_url.map(str::to_string),
            sample_url: None,
            thumb_url: None,
            created_at: None,
            post_url: format!("https://danbooru.donmai.us/posts/{id}"),
            tags: PostTags { artist: vec!["alice".into()], ..PostTags::default() },
            pages: None,
        }
    }

    #[test]
    fn folder_names_are_safe_everywhere() {
        assert_eq!(safe_name("kantoku"), "kantoku");
        assert_eq!(safe_name("a/b:c*?"), "a_b_c__");
        assert_eq!(safe_name("hello. "), "hello");
        assert_eq!(safe_name(".hack"), "_hack");
        assert_eq!(safe_name("con"), "_con");
        assert_eq!(safe_name("COM3.x"), "_COM3.x");
        assert_eq!(safe_name("compass"), "compass");
        assert_eq!(safe_name("..."), "_");
        assert_eq!(safe_name(&"長".repeat(150)).chars().count(), 100);
    }

    #[test]
    fn targets_group_by_site_and_artist() {
        let root = Path::new("/pics");
        let mut p = post(42, "png", None);
        assert_eq!(target_path(root, &p, "png"), PathBuf::from("/pics/Danbooru/alice/42.png"));
        p.tags.artist.clear();
        assert_eq!(target_path(root, &p, "png"), PathBuf::from("/pics/Danbooru/42.png"));
    }

    #[test]
    fn only_images_get_an_extension() {
        assert_eq!(image_ext(&post(1, "PNG", None)).as_deref(), Some("png"));
        assert_eq!(image_ext(&post(1, "mp4", None)), None);
        assert_eq!(image_ext(&post(1, "", Some("https://img4.gelbooru.com/images/a/b.jpeg"))).as_deref(), Some("jpeg"));
        assert_eq!(image_ext(&post(1, "", None)), None);
    }

    struct Harness {
        _dir: tempfile::TempDir,
        library: Library,
        downloader: Arc<Downloader>,
        events: Arc<Mutex<Vec<String>>>,
    }

    async fn harness() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let defaults = Defaults {
            images: dir.path().join("images"),
            data: dir.path().join("data"),
            cache: dir.path().join("cache"),
        };
        let storage = Storage::load(dir.path().join("storage.json"), defaults);
        let library = Library::in_memory().await;
        let events = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&events);
        let sink: EventSink = Arc::new(move |event| {
            let line = match event {
                Event::Job(job) => format!("job:{}:{:?}", job.id, job.status),
                Event::JobRemoved(id) => format!("removed:{id}"),
                Event::Saved { post_id, .. } => format!("saved:{post_id}"),
                Event::Subscription(sub) => format!("subscription:{}", sub.id),
                Event::NewPosts { title, saved } => format!("new:{title}:{saved}"),
            };
            log.lock().unwrap().push(line);
        });
        let downloader = Downloader::new(
            library.clone(),
            Arc::new(Net::new(&ProxySettings::default()).unwrap()),
            Arc::new(AccountStore::default()),
            Arc::new(RwLock::new(storage)),
            Arc::new(tokio::sync::RwLock::new(())),
            sink,
        );
        Harness { _dir: dir, library, downloader, events }
    }

    async fn wait_for(library: &Library, id: i64, status: JobStatus) -> JobInfo {
        for _ in 0..200 {
            let job = library.job(id).await.unwrap().unwrap();
            if job.status == status {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("任务没有变成 {status:?}");
    }

    #[tokio::test]
    async fn job_records_skips_and_failures_without_network() {
        let h = harness().await;
        // 已在图库中且文件还在。
        let owned_file = h._dir.path().join("owned.png");
        std::fs::write(&owned_file, b"png").unwrap();
        h.library.save_post(&post(1, "png", None), &owned_file, 1).await.unwrap();

        tokio::spawn(Arc::clone(&h.downloader).run());
        let posts = vec![
            post(1, "png", None),
            post(2, "mp4", Some("https://cdn.donmai.us/original/2.mp4")),
            post(3, "png", None),
            post(4, "png", Some("https://example.com/4.png")),
        ];
        let job = h.downloader.enqueue_posts(posts).await.unwrap();
        let job = wait_for(&h.library, job.id, JobStatus::Done).await;
        assert_eq!((job.saved, job.skipped, job.failed), (0, 2, 2));

        let notes = h.library.item_notes(job.id).await.unwrap();
        let notes: Vec<_> = notes.iter().map(|n| (n.post_id, n.note.clone().unwrap())).collect();
        assert_eq!(
            notes,
            vec![
                (1, note_owned().to_string()),
                (2, note_not_image().to_string()),
                (3, note_no_file(&post(3, "png", None)).to_string()),
                (4, note_bad_url().to_string()),
            ]
        );
        let events = h.events.lock().unwrap().clone();
        assert!(events.contains(&format!("job:{}:Running", job.id)));
        assert_eq!(events.last().unwrap(), &format!("job:{}:Done", job.id));
    }

    #[tokio::test]
    async fn queued_job_can_be_paused_resumed_and_removed() {
        let h = harness().await;
        // 不启动队列，任务停在排队状态。
        let job = h.downloader.enqueue_posts(vec![post(3, "png", None)]).await.unwrap();
        h.downloader.pause(job.id).await.unwrap();
        assert_eq!(h.library.job(job.id).await.unwrap().unwrap().status, JobStatus::Paused);

        h.downloader.resume(job.id).await.unwrap();
        tokio::spawn(Arc::clone(&h.downloader).run());
        let done = wait_for(&h.library, job.id, JobStatus::Done).await;
        assert_eq!(done.failed, 1);

        h.downloader.retry(job.id).await.unwrap();
        let done = wait_for(&h.library, job.id, JobStatus::Done).await;
        assert_eq!(done.failed, 1);

        h.downloader.remove(job.id).await.unwrap();
        assert!(h.library.job(job.id).await.unwrap().is_none());
        assert!(h.events.lock().unwrap().contains(&format!("removed:{}", job.id)));
    }
}
