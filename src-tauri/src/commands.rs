use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::error::AppError;
use crate::i18n::{self, text, tr, Language, LanguageSetting};
use crate::library::{
    Folder, GroupPage, GroupQuery, ItemNote, JobInfo, LibraryPage, LibraryQuery, NewSubscription, SavedSearch, Subscription,
};
use crate::settings::{parse_proxy_url, KeyStorage, ProxyMode, ProxySettings, SavedAccount};
use crate::sources::filter::{self, QueryPlan};
use crate::sources::{self, combined, danbooru, gelbooru, pixiv, Page, Post, Rating, SearchPage, SearchParams, Sort, Source};
use crate::storage::{self, ChangeMode, StorageInfo, StorageKind};
use crate::{keys, net, secrets, thumbs, x_bridge, AppState};

/// 瀑布流每页条数。下载任务另按站点上限（200 / 100）分页。
const PAGE_SIZE: u32 = 40;

/// Danbooru 一次能搜几个 tag：看当前实际登录的账号等级；其余站点不限。
fn tag_limit(state: &AppState, source: Source) -> Option<usize> {
    match source {
        Source::Danbooru => {
            let signed_in = state.accounts.get().danbooru.is_some();
            let level = state.settings().accounts.danbooru.as_ref().and_then(|a| a.level.clone());
            Some(filter::danbooru_tag_limit(if signed_in { level.as_deref() } else { None }))
        }
        Source::Gelbooru | Source::Yandere | Source::Pixiv | Source::X | Source::Custom => None,
    }
}

/// 超出 tag 上限时一次最多往下翻几页找够一页结果，免得条件太严时一直翻。
const MAX_FILTERED_PAGES: usize = 5;

/// 搜一个站点一页的结果。
struct SiteResults {
    posts: Vec<Post>,
    next: Option<String>,
    query: String,
    local_filter: String,
    /// 这一页翻过的图（本地筛选时连筛掉的也算）里按所选排序最靠后的位置，聚合搜索时用。
    reached: Option<combined::Rank>,
}

async fn search_with_plan(
    state: &AppState,
    params: &SearchParams,
    plan: &QueryPlan,
    page_size: u32,
) -> Result<SiteResults, AppError> {
    let accounts = state.accounts.get();
    let query = &plan.server_query;
    let (posts, next, reached) = if plan.local.is_empty() {
        let page = params.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(1u32).max(1);
        let (posts, fetched) =
            sources::fetch(&state.net, &accounts, params.source, query, &Page::Number(page), page_size).await?;
        let reached = combined::lowest(&posts, params.sort);
        (posts, (fetched >= page_size as usize).then(|| (page + 1).to_string()), reached)
    } else {
        // 按站点每页最多的条数往下翻，本地筛到够一页或翻满几页就先返回，剩下的下次接着翻。
        let mut page = params.cursor.as_deref().and_then(Page::parse).unwrap_or(Page::Number(1));
        let limit = params.source.max_page_size();
        let mut matched = Vec::new();
        let mut next = None;
        let mut reached = None;
        for _ in 0..MAX_FILTERED_PAGES {
            let (posts, fetched) = sources::fetch(&state.net, &accounts, params.source, query, &page, limit).await?;
            let ids = posts.iter().map(|post| post.id);
            let Some(bounds) = ids.clone().min().zip(ids.max()).filter(|_| fetched > 0) else {
                next = None;
                break;
            };
            reached = reached.into_iter().chain(combined::lowest(&posts, params.sort)).min();
            matched.extend(posts.into_iter().filter(|post| plan.local.matches(post)));
            page = page.next(params.source, query, Some(bounds));
            next = Some(page.to_param());
            if matched.len() >= page_size as usize {
                break;
            }
        }
        (matched, next, reached)
    };
    Ok(SiteResults { posts, next, query: query.clone(), local_filter: plan.local.to_query(), reached })
}

/// 搜一个站点的一页，每页 `page_size` 张。
async fn search_site(state: &AppState, params: &SearchParams, page_size: u32) -> Result<SiteResults, AppError> {
    let tags = params.tags_with_sort()?;
    let mut limit = tag_limit(state, params.source);
    // 站点实际的上限比按账号等级算的小时（例如等级刚变），按站点给的数字重新拆一次。
    for retry in [false, true] {
        let plan = filter::plan_query(params.source, &tags, &params.ratings, limit)?;
        match search_with_plan(state, params, &plan, page_size).await {
            Err(AppError::TagLimit { limit: actual, .. })
                if !retry && limit.is_some_and(|l| l > actual as usize) =>
            {
                limit = Some(actual as usize);
            }
            result => return result,
        }
    }
    unreachable!("第二次一定会返回")
}

#[tauri::command]
pub async fn search_remote(state: State<'_, AppState>, params: SearchParams) -> Result<SearchPage, AppError> {
    let found = search_site(&state, &params, PAGE_SIZE).await?;
    let owned = state.library.owned(&found.posts).await?.into_iter().map(|(_, post_id)| post_id).collect();
    Ok(SearchPage { posts: found.posts, next: found.next, query: found.query, local_filter: found.local_filter, owned })
}

/// 聚合搜索：同样的 tag、分级和排序，同时搜几个站点。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SitesSearchParams {
    pub sources: Vec<Source>,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub ratings: Vec<Rating>,
    #[serde(default)]
    pub sort: Sort,
    /// 上一次返回的 `next`；为空表示第一页。
    #[serde(default)]
    pub cursor: Option<String>,
}

/// 聚合搜索里一个站点这一页的情况。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteStatus {
    pub source: Source,
    pub query: String,
    pub local_filter: String,
    pub error: Option<AppError>,
    /// 出错的站点下一页还会再试（网络问题）；账号、条件不对时这个站点不再往下翻。
    pub retry: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SitesPage {
    pub posts: Vec<Post>,
    pub next: Option<String>,
    /// 这一页搜了的站点；有图在等着显示的站点这一页不用搜，不在里面。
    pub sites: Vec<SiteStatus>,
    pub owned: Vec<PostRef>,
}

/// 出错的站点也显示它的查询：按同样的规则拆一次，拆不出来就留空。
fn site_plan(state: &AppState, params: &SearchParams) -> (String, String) {
    params
        .tags_with_sort()
        .and_then(|tags| filter::plan_query(params.source, &tags, &params.ratings, tag_limit(state, params.source)))
        .map(|plan| (plan.server_query, plan.local.to_query()))
        .unwrap_or_default()
}

#[tauri::command]
pub async fn search_sites(state: State<'_, AppState>, params: SitesSearchParams) -> Result<SitesPage, AppError> {
    combined_search(&state, params).await
}

/// 每个站点各取一批（合起来和单个站点一页差不多），按所选排序合成一列，规则见 [`combined`]。
/// 一个站点出错不影响其他站点；第一页所有站点都出错时才整个报错。
async fn combined_search(state: &AppState, params: SitesSearchParams) -> Result<SitesPage, AppError> {
    let sources: Vec<Source> = Source::REMOTE.into_iter().filter(|source| params.sources.contains(source)).collect();
    if sources.is_empty() {
        return Err(AppError::InvalidInput(tr!("至少选一个平台", "Choose at least one site")));
    }
    if !combined::can_merge(params.sort) {
        return Err(AppError::InvalidInput(tr!(
            "聚合搜索只能按上传先后、分数或分辨率排序",
            "Combined search can only sort by upload date, score or resolution"
        )));
    }
    for source in &sources {
        params.sort.term(*source)?;
    }
    let mut cursor = match params.cursor.as_deref() {
        None => combined::Cursor::start(&sources),
        Some(text) => serde_json::from_str::<combined::Cursor>(text).map_err(|_| {
            AppError::InvalidInput(tr!("翻页位置无效，请重新搜索", "Invalid page position. Search again."))
        })?,
    };
    cursor.sites.retain(|site| sources.contains(&site.source));

    let page_size = PAGE_SIZE.div_ceil(sources.len() as u32);
    let searches: Vec<SearchParams> = cursor
        .due()
        .into_iter()
        .map(|site| SearchParams {
            source: site.source,
            tags: params.tags.clone(),
            ratings: params.ratings.clone(),
            sort: params.sort,
            cursor: site.page.clone(),
        })
        .collect();
    let results = futures_util::future::join_all(searches.into_iter().map(|search| async move {
        let result = search_site(state, &search, page_size).await;
        (search, result)
    }))
    .await;

    // 等着显示的图在前，这一页取到的接在各自站点后面。
    let mut pool = std::mem::take(&mut cursor.held);
    let mut sites = Vec::new();
    for (search, result) in results {
        let source = search.source;
        match result {
            Ok(found) => {
                cursor.advance(source, found.next, found.reached);
                pool.extend(found.posts);
                sites.push(SiteStatus {
                    source,
                    query: found.query,
                    local_filter: found.local_filter,
                    error: None,
                    retry: false,
                });
            }
            Err(err) => {
                // 网络问题时下一页再试同一页；账号、条件不对时这个站点不再往下翻。
                let retry = err.is_transient();
                if !retry {
                    cursor.stop(source);
                }
                let (query, local_filter) = site_plan(state, &search);
                sites.push(SiteStatus { source, query, local_filter, error: Some(err), retry });
            }
        }
    }
    if params.cursor.is_none() && sites.iter().all(|site| site.error.is_some()) {
        if let Some(err) = sites.iter_mut().find_map(|site| site.error.take()) {
            return Err(err);
        }
    }

    let mut queues = combined::by_site(&sources, pool);
    let posts = combined::take_ready(&mut queues, cursor.bar(), params.sort);
    cursor.held = queues.into_iter().flatten().collect();
    let owned = state.library.owned(&posts).await?;
    let next = if cursor.is_done() {
        None
    } else {
        Some(serde_json::to_string(&cursor).map_err(|err| AppError::Internal(err.to_string()))?)
    };
    Ok(SitesPage {
        posts,
        next,
        sites,
        owned: owned.into_iter().map(|(source, post_id)| PostRef { source, post_id }).collect(),
    })
}

/// 查询条件一共能搜到多少张，下载全部结果前给用户确认。
/// Danbooru 的计数接口不限 tag 数量，所以用完整条件，超出上限时也准确。
#[tauri::command]
pub async fn count_remote(state: State<'_, AppState>, params: SearchParams) -> Result<Option<u64>, AppError> {
    let query = sources::build_query(params.source, &params.tags_for_count()?, &params.ratings);
    sources::count(&state.net, &state.accounts.get(), params.source, &query).await
}

/// 下载选中的图。来自几个站点时（聚合搜索）每个站点各建一个任务。
#[tauri::command]
pub async fn download_posts(state: State<'_, AppState>, posts: Vec<Post>) -> Result<Vec<JobInfo>, AppError> {
    let mut jobs = Vec::new();
    let mut rest = posts;
    for source in Source::ALL {
        let (group, others): (Vec<Post>, Vec<Post>) = rest.into_iter().partition(|post| post.source == source);
        rest = others;
        if !group.is_empty() {
            jobs.push(state.downloader.enqueue_posts(group).await?);
        }
    }
    Ok(jobs)
}

/// 按条件下载全部结果；`max_posts` 限制最多下载前多少张。
#[tauri::command]
pub async fn download_query(
    state: State<'_, AppState>,
    params: SearchParams,
    max_posts: Option<u32>,
) -> Result<JobInfo, AppError> {
    // 按所选排序翻页，设了上限时就是「排在前面的 N 张」。
    let tags = params.tags_with_sort()?;
    let plan = filter::plan_query(params.source, &tags, &params.ratings, tag_limit(&state, params.source))?;
    let count_query = sources::build_query(params.source, &params.tags_for_count()?, &params.ratings);
    let words = params.tags.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if words.is_empty() { text("全部帖子", "All posts").to_string() } else { words };
    let local = plan.local.to_query();
    state
        .downloader
        .enqueue_query(params.source, &title, &plan.server_query, Some(&local), &count_query, max_posts.map(i64::from))
        .await
}

#[tauri::command]
pub async fn list_jobs(state: State<'_, AppState>) -> Result<Vec<JobInfo>, AppError> {
    Ok(state.library.jobs().await?)
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobAction {
    Pause,
    Resume,
    Cancel,
    Retry,
    Remove,
}

#[tauri::command]
pub async fn job_action(state: State<'_, AppState>, id: i64, action: JobAction) -> Result<(), AppError> {
    let downloader = &state.downloader;
    match action {
        JobAction::Pause => downloader.pause(id).await,
        JobAction::Resume => downloader.resume(id).await,
        JobAction::Cancel => downloader.cancel(id).await,
        JobAction::Retry => downloader.retry(id).await,
        JobAction::Remove => downloader.remove(id).await,
    }
}

/// 任务里失败和跳过的图及原因。
#[tauri::command]
pub async fn job_notes(state: State<'_, AppState>, id: i64) -> Result<Vec<ItemNote>, AppError> {
    Ok(state.library.item_notes(id).await?)
}

#[tauri::command]
pub async fn clear_finished_jobs(state: State<'_, AppState>) -> Result<(), AppError> {
    state.downloader.clear_finished().await
}

#[tauri::command]
pub async fn library_list(state: State<'_, AppState>, query: LibraryQuery) -> Result<LibraryPage, AppError> {
    let mut page = state.library.list(&query).await?;
    for post in &mut page.posts {
        post.missing = !tokio::fs::try_exists(&post.path).await.unwrap_or(false);
    }
    Ok(page)
}

/// 图库首页的文件夹（按来源）。
#[tauri::command]
pub async fn library_folders(state: State<'_, AppState>) -> Result<Vec<Folder>, AppError> {
    Ok(state.library.folders().await?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub imported: u32,
    pub skipped: u32,
}

struct ImportedFile {
    post: Post,
    path: PathBuf,
}

fn import_id(md5: &str) -> u64 {
    let mut bytes = [0u8; 8];
    for (index, pair) in md5.as_bytes().chunks(2).take(8).enumerate() {
        bytes[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0);
    }
    let id = u64::from_be_bytes(bytes) & 0x7fff_ffff_ffff_ffff;
    if id == 0 { 1 } else { id }
}

fn import_files(paths: Vec<PathBuf>, root: PathBuf) -> Result<(Vec<ImportedFile>, u32), AppError> {
    let mut imported = Vec::new();
    let mut skipped = 0;
    for source in paths {
        let Some(file_name) = source.file_name().map(|name| name.to_string_lossy().into_owned()) else {
            skipped += 1;
            continue;
        };
        let Ok(bytes) = std::fs::read(&source) else {
            skipped += 1;
            continue;
        };
        let Ok(reader) = image::ImageReader::open(&source) else {
            skipped += 1;
            continue;
        };
        let Ok(reader) = reader.with_guessed_format() else {
            skipped += 1;
            continue;
        };
        let Ok(image) = reader.decode() else {
            skipped += 1;
            continue;
        };
        let digest = Md5::digest(&bytes);
        let md5: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let parent = source.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "导入".into());
        let ext = source.extension().and_then(|ext| ext.to_str()).unwrap_or("jpg").to_ascii_lowercase();
        let target = root
            .join(Source::Custom.site_name())
            .join(crate::downloader::safe_name(&parent))
            .join(crate::downloader::safe_name(&file_name));
        if source != target {
            if let Some(parent) = target.parent() { std::fs::create_dir_all(parent).map_err(|err| AppError::Internal(err.to_string()))?; }
            std::fs::copy(&source, &target).map_err(|err| AppError::Internal(err.to_string()))?;
        }
        imported.push(ImportedFile {
            post: Post {
                source: Source::Custom,
                id: import_id(&md5),
                md5: Some(md5),
                width: image.width(),
                height: image.height(),
                rating: None,
                score: 0,
                fav_count: None,
                file_ext: ext,
                file_size: Some(bytes.len() as u64),
                file_url: None,
                sample_url: None,
                thumb_url: None,
                created_at: None,
                post_url: format!("file://{}", source.to_string_lossy()),
                tags: sources::PostTags { artist: vec![parent], ..sources::PostTags::default() },
                pages: None,
            },
            path: target,
        });
    }
    Ok((imported, skipped))
}

#[tauri::command]
pub async fn library_import(app: AppHandle, state: State<'_, AppState>, paths: Vec<String>) -> Result<ImportOutcome, AppError> {
    if paths.is_empty() {
        return Err(AppError::InvalidInput(tr!("没有选择图片", "No images selected")));
    }
    let root = state.storage().path(StorageKind::Images);
    let (files, skipped) = blocking(move || import_files(paths.into_iter().map(PathBuf::from).collect(), root)).await?;
    let mut imported = 0;
    for file in files {
        let id = file.post.id;
        state.library.save_post(&file.post, &file.path, crate::library::now_ms()).await?;
        let _ = app.emit("library-changed", serde_json::json!({ "source": "custom", "postId": id }));
        imported += 1;
    }
    Ok(ImportOutcome { imported, skipped })
}

/// 文件夹里按画师、作品、角色或 tag 分的组。
#[tauri::command]
pub async fn library_groups(state: State<'_, AppState>, query: GroupQuery) -> Result<GroupPage, AppError> {
    Ok(state.library.groups(&query).await?)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostRef {
    source: Source,
    post_id: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFailure {
    post_id: u64,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOutcome {
    removed: Vec<PostRef>,
    failed: Vec<DeleteFailure>,
}

/// 从图库删除。`keep_files` 为 false 时先把图片移到废纸篓（回收站），移不走的保留记录并报告原因；
/// 文件本来就不在的直接删记录。缩略图随记录一起删掉。
#[tauri::command]
pub async fn library_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    posts: Vec<PostRef>,
    keep_files: bool,
) -> Result<DeleteOutcome, AppError> {
    // 和移动图片位置互斥，免得删到一半文件被搬走。
    let _gate = state.images_gate.read().await;
    let mut targets = Vec::with_capacity(posts.len());
    for post in posts {
        if let Some(path) = state.library.local_path(post.source, post.post_id).await? {
            targets.push((post, path));
        }
    }
    let cache = state.storage().path(StorageKind::Cache);
    let (removed, failed) = blocking(move || {
        let mut removed = Vec::new();
        let mut failed = Vec::new();
        for (post, path) in targets {
            let result = if keep_files { Ok(()) } else { storage::move_to_trash(&path) };
            match result {
                Ok(()) => {
                    let _ = std::fs::remove_file(thumbs::path(&cache, post.source, post.post_id));
                    removed.push(post);
                }
                Err(message) => failed.push(DeleteFailure { post_id: post.post_id, message }),
            }
        }
        Ok((removed, failed))
    })
    .await?;
    let keys: Vec<(Source, u64)> = removed.iter().map(|post| (post.source, post.post_id)).collect();
    state.library.remove_posts(&keys).await?;
    let _ = app.emit("library-removed", &removed);
    Ok(DeleteOutcome { removed, failed })
}

fn join_error(err: tauri::Error) -> AppError {
    AppError::Internal(err.to_string())
}

// ---------- 订阅 ----------

/// 检查间隔的下限：太频繁对站点不友好，新图也不会那么快出现。
const MIN_INTERVAL_MINUTES: u32 = 30;

fn interval_too_short() -> AppError {
    AppError::InvalidInput(tr!(
        "检查间隔至少 {MIN_INTERVAL_MINUTES} 分钟",
        "The check interval must be at least {MIN_INTERVAL_MINUTES} minutes"
    ))
}

#[tauri::command]
pub async fn subscriptions_list(state: State<'_, AppState>) -> Result<Vec<Subscription>, AppError> {
    Ok(state.library.subscriptions().await?)
}

// ---------- 收藏的搜索 ----------

#[tauri::command]
pub async fn saved_searches_list(state: State<'_, AppState>) -> Result<Vec<SavedSearch>, AppError> {
    Ok(state.library.saved_searches().await?)
}

/// 收藏的条件：和搜索一样，只是可以有几个站点（聚合搜索）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSearchParams {
    pub sources: Vec<Source>,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub ratings: Vec<Rating>,
    #[serde(default)]
    pub sort: Sort,
}

/// 收藏当前的搜索条件，返回收藏后的列表。
#[tauri::command]
pub async fn saved_search_add(
    state: State<'_, AppState>,
    params: SavedSearchParams,
) -> Result<Vec<SavedSearch>, AppError> {
    if params.sources.is_empty() {
        return Err(AppError::InvalidInput(tr!("至少选一个平台", "Choose at least one site")));
    }
    for source in &params.sources {
        params.sort.term(*source)?;
    }
    state.library.add_saved_search(&params.sources, &params.tags, &params.ratings, params.sort).await?;
    Ok(state.library.saved_searches().await?)
}

#[tauri::command]
pub async fn saved_search_remove(state: State<'_, AppState>, id: i64) -> Result<Vec<SavedSearch>, AppError> {
    state.library.remove_saved_search(id).await?;
    Ok(state.library.saved_searches().await?)
}

/// 订阅对话框里显示的条件：订阅按上传先后找新图、不带排序，超出 tag 上限时的拆分可能和当前搜索不一样。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionPreview {
    pub query: String,
    pub local_filter: String,
}

#[tauri::command]
pub async fn subscription_preview(
    state: State<'_, AppState>,
    params: SearchParams,
) -> Result<SubscriptionPreview, AppError> {
    let plan = filter::plan_query(params.source, &params.tags, &params.ratings, tag_limit(&state, params.source))?;
    Ok(SubscriptionPreview { query: plan.server_query, local_filter: plan.local.to_query() })
}

/// 订阅搜索条件。以现在最新的一张为起点，以后比它新的才下载；
/// `download_existing` 为 true 时顺便把现有的结果按「下载全部结果」加入队列（`max_posts` 限制张数）。
#[tauri::command]
pub async fn subscription_create(
    state: State<'_, AppState>,
    params: SearchParams,
    interval_minutes: u32,
    download_existing: bool,
    max_posts: Option<u32>,
) -> Result<Subscription, AppError> {
    let plan = filter::plan_query(params.source, &params.tags, &params.ratings, tag_limit(&state, params.source))?;
    let query = plan.server_query.clone();
    if sources::has_custom_order(&query) {
        return Err(AppError::InvalidInput(tr!(
            "订阅按上传先后找新图，条件里不能带 order: 或 sort: 这类排序",
            "Subscriptions find new posts by upload time, so the search can't include sorting like order: or sort:"
        )));
    }
    if interval_minutes < MIN_INTERVAL_MINUTES {
        return Err(interval_too_short());
    }
    // 顺便验证条件能搜（tag 数量、账号），出错直接提示，不建订阅。
    let accounts = state.accounts.get();
    let (posts, _) = sources::fetch(&state.net, &accounts, params.source, &query, &Page::Number(1), 1).await?;
    let newest = posts.iter().map(|post| post.id as i64).max().unwrap_or(0);
    let sub = state
        .library
        .create_subscription(NewSubscription {
            source: params.source,
            tags: &params.tags.split_whitespace().collect::<Vec<_>>().join(" "),
            ratings: &params.ratings,
            query: &query,
            interval_minutes: i64::from(interval_minutes),
            last_seen_id: newest,
            local_filter: Some(&plan.local.to_query()),
        })
        .await?;
    if download_existing {
        let full = sources::build_query(params.source, &params.tags, &params.ratings);
        let local = plan.local.to_query();
        state
            .downloader
            .enqueue_query(params.source, &sub.title(), &query, Some(&local), &full, max_posts.map(i64::from))
            .await?;
    }
    state.downloader.reschedule();
    Ok(sub)
}

#[tauri::command]
pub async fn subscription_update(
    state: State<'_, AppState>,
    id: i64,
    enabled: Option<bool>,
    interval_minutes: Option<u32>,
) -> Result<Subscription, AppError> {
    if interval_minutes.is_some_and(|m| m < MIN_INTERVAL_MINUTES) {
        return Err(interval_too_short());
    }
    let sub = state
        .library
        .update_subscription(id, enabled, interval_minutes.map(i64::from))
        .await?
        .ok_or_else(|| AppError::Internal(tr!("订阅不存在", "This subscription no longer exists")))?;
    state.downloader.reschedule();
    Ok(sub)
}

/// 删除订阅；已经下载的图和进行中的检查任务都保留。
#[tauri::command]
pub async fn subscription_delete(state: State<'_, AppState>, id: i64) -> Result<(), AppError> {
    state.library.delete_subscription(id).await?;
    Ok(())
}

/// 立即检查。已有检查任务在队列里时不重复建，返回 `None`。
#[tauri::command]
pub async fn subscription_check(state: State<'_, AppState>, id: i64) -> Result<Option<JobInfo>, AppError> {
    state.downloader.check_subscription(id).await
}

#[tauri::command]
pub async fn subscriptions_check_all(state: State<'_, AppState>) -> Result<usize, AppError> {
    state.downloader.check_all_subscriptions().await
}

// ---------- 通用 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralInfo {
    close_to_tray: bool,
    launch_at_login: bool,
    language: LanguageSetting,
    /// 实际使用的语言（跟随系统时是系统语言对应的那一种）。
    resolved_language: Language,
    log_file: PathBuf,
}

fn general_info_of(app: &AppHandle, state: &AppState) -> GeneralInfo {
    use tauri_plugin_autostart::ManagerExt;
    let (close_to_tray, language) = {
        let settings = state.settings();
        (settings.close_to_tray, settings.language)
    };
    GeneralInfo {
        close_to_tray,
        launch_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        language,
        resolved_language: i18n::current(),
        log_file: crate::log_file(&state.storage().path(StorageKind::Data)),
    }
}

/// 界面启动时先问用哪种语言，再开始渲染。
#[tauri::command]
pub fn language_current() -> Language {
    i18n::current()
}

#[tauri::command]
pub fn general_info(app: AppHandle, state: State<'_, AppState>) -> GeneralInfo {
    general_info_of(&app, &state)
}

#[tauri::command]
pub fn general_save(
    app: AppHandle,
    state: State<'_, AppState>,
    close_to_tray: bool,
    launch_at_login: bool,
    language: LanguageSetting,
) -> Result<GeneralInfo, AppError> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    if autostart.is_enabled().unwrap_or(false) != launch_at_login {
        let result = if launch_at_login { autostart.enable() } else { autostart.disable() };
        result.map_err(|e| AppError::Internal(tr!("设置开机启动失败：{e}", "Couldn't change launch at login: {e}")))?;
    }
    let changed = state.settings().language != language;
    state.update_settings(|settings| {
        settings.close_to_tray = close_to_tray;
        settings.language = language;
    })?;
    if changed {
        i18n::set(language.resolve());
        crate::refresh_menus(&app);
    }
    Ok(general_info_of(&app, &state))
}

// ---------- 账号 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    source: Source,
    /// 用户名（Gelbooru 是 User ID）；未登录时为空。
    name: Option<String>,
    level: Option<String>,
    /// 设置里记着账号，但钥匙串里找不到 API Key（例如换了电脑、钥匙串被清理）。
    key_missing: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsInfo {
    accounts: Vec<AccountView>,
    /// 新保存的 API Key 放在哪。
    key_storage: KeyStorage,
    /// 启动时读取 API Key 失败的原因。
    error: Option<String>,
}

fn accounts_info_of(state: &AppState) -> AccountsInfo {
    let settings = state.settings();
    let accounts = state.accounts.get();
    let view = |source: Source, saved: &Option<SavedAccount>, loaded: bool| AccountView {
        source,
        name: saved.as_ref().map(|a| a.name.clone()),
        level: saved.as_ref().and_then(|a| a.level.clone()),
        key_missing: saved.is_some() && !loaded,
    };
    AccountsInfo {
        accounts: vec![
            view(Source::Danbooru, &settings.accounts.danbooru, accounts.danbooru.is_some()),
            view(Source::Gelbooru, &settings.accounts.gelbooru, accounts.gelbooru.is_some()),
            view(Source::Pixiv, &settings.accounts.pixiv, accounts.pixiv.is_some()),
        ],
        key_storage: settings.key_storage,
        error: state.accounts_error(),
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(join_error)?
}

#[tauri::command]
pub fn accounts_info(state: State<'_, AppState>) -> AccountsInfo {
    accounts_info_of(&state)
}

/// 先用填写的账号访问一次站点，通过了才把 API Key 存进钥匙串。
/// Pixiv 填的是登录后的 PHPSESSID，账号名从站点取，不用填。
#[tauri::command]
pub async fn account_save(
    state: State<'_, AppState>,
    source: Source,
    name: String,
    api_key: String,
) -> Result<AccountsInfo, AppError> {
    save_account(&state, source, name, api_key).await
}

async fn save_account(state: &AppState, source: Source, name: String, api_key: String) -> Result<AccountsInfo, AppError> {
    let name = name.trim().to_string();
    let api_key = api_key.trim().to_string();
    if name.is_empty() && matches!(source, Source::Danbooru | Source::Gelbooru) {
        return Err(AppError::InvalidInput(if source == Source::Danbooru {
            tr!("请填写用户名", "Enter your username")
        } else {
            tr!("请填写 User ID", "Enter your User ID")
        }));
    }
    if api_key.is_empty() {
        return Err(AppError::InvalidInput(if source == Source::Pixiv {
            tr!("请粘贴 PHPSESSID", "Paste your PHPSESSID")
        } else {
            tr!("请填写 API Key", "Enter your API key")
        }));
    }
    let (name, level, api_key) = match source {
        Source::Danbooru => {
            let creds = danbooru::Credentials { username: name.clone(), api_key: api_key.clone() };
            let level = danbooru::verify(&state.net, &creds).await?.level_string;
            (name, level, api_key)
        }
        Source::Gelbooru => {
            let creds = gelbooru::Credentials { user_id: name.clone(), api_key: api_key.clone() };
            gelbooru::verify(&state.net, &creds).await?;
            (name, None, api_key)
        }
        Source::Pixiv => {
            let creds = pixiv::Credentials::from_session(&api_key).ok_or_else(|| {
                AppError::InvalidInput(tr!(
                    "这不是登录后的 PHPSESSID，它应该是「数字_字母」的样子",
                    "That isn't a signed-in PHPSESSID. It should look like digits_letters"
                ))
            })?;
            let user = pixiv::verify(&state.net, &creds).await?;
            (user, None, creds.session)
        }
        Source::Yandere => {
            return Err(AppError::InvalidInput(tr!("Yande.re 不需要账号", "Yande.re doesn't need an account")))
        }
        Source::X => {
            return Err(AppError::InvalidInput(tr!("X 的登录在媒体采集窗口里完成", "Sign in to X in the media capture window")))
        }
        Source::Custom => {
            return Err(AppError::InvalidInput(tr!("自定义导入不能填写账号", "Custom imports don't use an account")))
        }
    };

    let snapshot = state.settings().clone();
    let previous = snapshot.accounts.get(source).cloned();
    let (key_name, key) = (name.clone(), api_key.clone());
    let (key_salt, sealed_key) = blocking(move || {
        let mut draft = snapshot;
        let sealed = keys::store(&mut draft, source, &key_name, &key)?;
        // 换了账号时删掉旧账号的 Key，删不掉也不影响新账号使用。
        if let Some(old) = previous.filter(|old| old.name != key_name) {
            let _ = keys::forget(source, &old);
        }
        Ok((draft.key_salt, sealed))
    })
    .await?;

    state.update_settings(|settings| {
        settings.key_salt = key_salt;
        settings.accounts.set(source, Some(SavedAccount { name: name.clone(), level, sealed_key }));
    })?;
    state.accounts.update(|accounts| accounts.set(source, Some((name, api_key))));
    state.clear_accounts_error();
    Ok(accounts_info_of(state))
}

/// 退出登录：删掉保存的 API Key 和设置里的用户名。Pixiv 还要清掉登录窗口留下的 Cookie，
/// 不然下次点「登录」会直接用上一个账号登录。
#[tauri::command]
pub async fn account_remove(app: AppHandle, state: State<'_, AppState>, source: Source) -> Result<AccountsInfo, AppError> {
    let saved = state.settings().accounts.get(source).cloned();
    if let Some(saved) = saved {
        blocking(move || keys::forget(source, &saved)).await?;
    }
    state.update_settings(|settings| settings.accounts.set(source, None))?;
    state.accounts.update(|accounts| accounts.set(source, None));
    if source == Source::Pixiv {
        forget_pixiv_cookies(&app);
    }
    Ok(accounts_info_of(&state))
}

const PIXIV_LOGIN_WINDOW: &str = "pixiv-login";

fn webview_proxy(proxy: &ProxySettings) -> Result<Option<url::Url>, AppError> {
    if proxy.mode != ProxyMode::Manual {
        return Ok(None);
    }
    let mut url = parse_proxy_url(&proxy.url)?;
    if url.scheme() == "socks5h" {
        url.set_scheme("socks5").map_err(|_| AppError::InvalidInput(tr!(
            "登录窗口不支持这个代理地址",
            "The login window doesn't support this proxy address"
        )))?;
    }
    if !matches!(url.scheme(), "http" | "socks5") {
        return Err(AppError::InvalidInput(tr!(
            "Pixiv 登录窗口只支持 http 或 socks5 代理",
            "The Pixiv login window supports http or socks5 proxies"
        )));
    }
    Ok(Some(url))
}

fn x_page(username: &str) -> Result<url::Url, AppError> {
    let username = username.trim().trim_start_matches('@');
    if username.is_empty() || username.len() > 15 || !username.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        return Err(AppError::InvalidInput(tr!("请输入有效的 X 用户名", "Enter a valid X username")));
    }
    format!("https://x.com/{username}/media")
        .parse()
        .map_err(|err: url::ParseError| AppError::Internal(err.to_string()))
}

/// 打开 X 媒体采集窗口。登录状态只留在这个窗口自己的 WebView Cookie 中。
#[tauri::command]
pub async fn x_capture_open(app: AppHandle, username: String) -> Result<(), AppError> {
    let url = x_page(&username)?;
    if let Some(window) = app.get_webview_window(x_bridge::WINDOW) {
        let _ = window.set_focus();
        window.navigate(url).map_err(|err| AppError::Internal(err.to_string()))?;
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, x_bridge::WINDOW, WebviewUrl::External(url))
        .title(tr!("X 媒体采集", "X media capture"))
        .inner_size(1100.0, 760.0)
        .initialization_script(x_bridge::INIT_SCRIPT)
        .on_navigation(|url| {
            url.scheme() == "https"
                && url.host_str().and_then(Source::for_host) == Some(Source::X)
        })
        .build()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn x_capture_close(app: AppHandle) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window(x_bridge::WINDOW) {
        window.close().map_err(|err| AppError::Internal(err.to_string()))?;
    }
    Ok(())
}

fn pixiv_site() -> url::Url {
    url::Url::parse(pixiv::REFERER_URL).expect("固定的地址")
}

/// 所有窗口共用一份 Cookie，从主窗口删掉 Pixiv 的登录 Cookie 就行。
fn forget_pixiv_cookies(app: &AppHandle) {
    let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) else { return };
    let Ok(cookies) = window.cookies_for_url(pixiv_site()) else { return };
    for cookie in cookies.into_iter().filter(|cookie| cookie.name() == "PHPSESSID") {
        let _ = window.delete_cookie(cookie);
    }
}

/// 打开 Pixiv 的登录页。账号密码只在 Pixiv 自己的页面里输入，这个窗口没有调用软件功能的权限；
/// 登录成功后由 [`pixiv_login_check`] 从窗口的 Cookie 里取出登录状态。
#[tauri::command]
pub async fn pixiv_login_open(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    if let Some(window) = app.get_webview_window(PIXIV_LOGIN_WINDOW) {
        let _ = window.set_focus();
        return Ok(());
    }
    let url = "https://accounts.pixiv.net/login?return_to=https%3A%2F%2Fwww.pixiv.net%2F&source=pc&view_type=page";
    let url = url.parse().map_err(|err: url::ParseError| AppError::Internal(err.to_string()))?;
    let proxy = webview_proxy(&state.settings().proxy)?;
    let mut builder = WebviewWindowBuilder::new(&app, PIXIV_LOGIN_WINDOW, WebviewUrl::External(url))
        .title(tr!("登录 Pixiv", "Sign in to Pixiv"))
        .inner_size(480.0, 720.0);
    if let Some(proxy) = proxy {
        builder = builder.proxy_url(proxy);
    }
    builder
        .build()
        .map_err(|err| AppError::Internal(err.to_string()))?;
    Ok(())
}

/// 登录窗口现在的情况。
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PixivLogin {
    /// 窗口还开着，还没登录好。
    Waiting,
    /// 窗口已经关掉了（登录成功后软件自己关的除外）。
    Closed,
    /// 登录成功，账号已保存，窗口已关掉。
    SignedIn { info: AccountsInfo },
}

/// 登录窗口里是否已经登录：读到登录后的 PHPSESSID 就验证、保存并关掉窗口。界面每隔一会儿问一次。
/// Windows 上读 Cookie 不能在主线程，所以这是异步命令。
#[tauri::command]
pub async fn pixiv_login_check(app: AppHandle, state: State<'_, AppState>) -> Result<PixivLogin, AppError> {
    let Some(window) = app.get_webview_window(PIXIV_LOGIN_WINDOW) else { return Ok(PixivLogin::Closed) };
    let cookies = window.cookies_for_url(pixiv_site()).map_err(|err| AppError::Internal(err.to_string()))?;
    let Some(session) = cookies
        .iter()
        .filter(|cookie| cookie.name() == "PHPSESSID")
        .find_map(|cookie| pixiv::Credentials::from_session(cookie.value()))
    else {
        return Ok(PixivLogin::Waiting);
    };
    match save_account(&state, Source::Pixiv, String::new(), session.session).await {
        Ok(info) => {
            let _ = window.close();
            Ok(PixivLogin::SignedIn { info })
        }
        // 上次登录留下的 Cookie 已经失效：用户正在窗口里重新登录，接着等。
        Err(AppError::BadCredentials { .. }) => Ok(PixivLogin::Waiting),
        Err(err) => Err(err),
    }
}

/// 切换 API Key 的保存方式，已保存的 Key 一起搬过去。
#[tauri::command]
pub async fn account_key_storage(state: State<'_, AppState>, storage: KeyStorage) -> Result<AccountsInfo, AppError> {
    let snapshot = state.settings().clone();
    if snapshot.key_storage == storage {
        return Ok(accounts_info_of(&state));
    }
    let accounts = state.accounts.get();
    let migration = blocking(move || keys::migrate(&snapshot, &accounts, storage)).await?;
    let stale = migration.stale.clone();
    state.update_settings(|settings| {
        settings.key_storage = storage;
        settings.key_salt = migration.key_salt;
        settings.accounts = migration.accounts;
    })?;
    // 新位置已经写好、设置也保存了，再删钥匙串里的旧项；删不掉不影响使用。
    blocking(move || {
        for (source, name) in stale {
            let _ = secrets::delete(source, &name);
        }
        Ok(())
    })
    .await?;
    Ok(accounts_info_of(&state))
}

// ---------- 网络 ----------

#[tauri::command]
pub fn proxy_info(state: State<'_, AppState>) -> ProxySettings {
    state.settings().proxy.clone()
}

/// 保存代理设置并立即生效。
#[tauri::command]
pub fn proxy_save(app: AppHandle, state: State<'_, AppState>, proxy: ProxySettings) -> Result<ProxySettings, AppError> {
    proxy.validate()?;
    state.net.apply_proxy(&proxy)?;
    state.update_settings(|settings| settings.proxy = proxy.clone())?;
    // WebView 的代理只能在创建窗口时设置；让下一次 Pixiv 登录使用新代理。
    if let Some(window) = app.get_webview_window(PIXIV_LOGIN_WINDOW) {
        let _ = window.close();
    }
    Ok(proxy)
}

/// 用还没保存的代理设置试连一次 Danbooru，返回耗时（毫秒）。
#[tauri::command]
pub async fn proxy_test(proxy: ProxySettings) -> Result<u64, AppError> {
    proxy.validate()?;
    Ok(net::test_connection(&proxy).await?.as_millis() as u64)
}

#[tauri::command]
pub fn storage_info(state: State<'_, AppState>) -> StorageInfo {
    state.storage().info()
}

/// 统计目录占用。图片多时要遍历很多文件，放到阻塞线程里做。
#[tauri::command]
pub async fn storage_usage(state: State<'_, AppState>, kind: StorageKind) -> Result<u64, AppError> {
    let path = state.storage().path(kind);
    tauri::async_runtime::spawn_blocking(move || storage::dir_size(&path)).await.map_err(join_error)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeOutcome {
    /// `true`：已生效；`false`：重启后生效。
    applied: bool,
    info: StorageInfo,
}

#[tauri::command]
pub async fn storage_change(
    state: State<'_, AppState>,
    kind: StorageKind,
    path: Option<String>,
    mode: ChangeMode,
) -> Result<ChangeOutcome, AppError> {
    // 移动图片期间暂停保存新下载的图（进行中的几张先存完），免得新图落进正在搬走的目录。
    let _gate = match kind {
        StorageKind::Images => Some(state.images_gate.write().await),
        _ => None,
    };
    let plan = state.storage().plan(kind, path.map(PathBuf::from), mode)?;
    // 移动在锁外进行：搬大量图片时，图片协议仍能照常读取当前位置。
    let plan = if kind.applies_on_restart() {
        plan
    } else {
        tauri::async_runtime::spawn_blocking(move || plan.execute().map(|_| plan)).await.map_err(join_error)??
    };
    let (applied, info) = {
        let mut storage = state.storage_mut();
        let applied = storage.commit(&plan)?;
        (applied, storage.info())
    };
    if kind == StorageKind::Images {
        if let Some((from, to)) = plan.moved() {
            state.library.rebase_paths(from, to).await?;
        }
    }
    Ok(ChangeOutcome { applied, info })
}

#[tauri::command]
pub fn storage_cancel_pending(state: State<'_, AppState>, kind: StorageKind) -> Result<StorageInfo, AppError> {
    let mut storage = state.storage_mut();
    storage.cancel_pending(kind)?;
    Ok(storage.info())
}

#[tauri::command]
pub fn storage_dismiss_error(state: State<'_, AppState>) -> Result<StorageInfo, AppError> {
    let mut storage = state.storage_mut();
    storage.dismiss_error()?;
    Ok(storage.info())
}

/// 确保目录存在并返回路径，供「在访达中显示」使用（图片目录在第一次下载前可能还没创建）。
#[tauri::command]
pub fn storage_prepare(state: State<'_, AppState>, kind: StorageKind) -> Result<String, AppError> {
    let path = state.storage().prepare(kind).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use image::{Rgb, RgbImage};
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex, RwLock};

    use super::*;
    use crate::downloader::{Downloader, EventSink};
    use crate::library::Library;
    use crate::net::Net;
    use crate::settings::Settings;
    use crate::sources::AccountStore;
    use crate::storage::{Defaults, Storage};

    async fn state(dir: &std::path::Path) -> AppState {
        let net = Arc::new(Net::new(&ProxySettings::default()).unwrap());
        let accounts = Arc::new(AccountStore::default());
        let library = Library::in_memory().await;
        let defaults = Defaults { images: dir.join("images"), data: dir.join("data"), cache: dir.join("cache") };
        let storage = Arc::new(RwLock::new(Storage::load(dir.join("storage.json"), defaults)));
        let images_gate = Arc::new(tokio::sync::RwLock::new(()));
        let events: EventSink = Arc::new(|_| {});
        let downloader = Downloader::new(
            library.clone(),
            Arc::clone(&net),
            Arc::clone(&accounts),
            Arc::clone(&storage),
            Arc::clone(&images_gate),
            events,
        );
        AppState {
            net,
            accounts,
            library,
            downloader,
            images_gate,
            storage,
            settings: Mutex::new(Settings::default()),
            accounts_error: Mutex::new(None),
        }
    }

    #[test]
    fn imports_images_into_custom_folder() {
        let root = tempfile::tempdir().unwrap();
        let source_dir = root.path().join("artist");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("sample.png");
        RgbImage::from_pixel(12, 8, Rgb([20, 40, 60])).save(&source).unwrap();
        let target_root = root.path().join("images");
        let (files, skipped) = import_files(vec![source], target_root.clone()).unwrap();
        assert_eq!(skipped, 0);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].post.source, Source::Custom);
        assert_eq!(files[0].post.tags.artist, ["artist"]);
        assert_eq!((files[0].post.width, files[0].post.height), (12, 8));
        assert!(files[0].path.starts_with(target_root.join("自定义导入/artist")));
        assert!(files[0].path.exists());
    }

    /// 未登录时搜两个站点：Gelbooru 报缺账号、之后不再搜它，Danbooru 照常往下翻，几页接起来从新到旧。
    #[tokio::test]
    #[ignore = "需要网络，手动运行"]
    async fn combined_search_on_real_sites() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path()).await;
        let params = |cursor| SitesSearchParams {
            sources: Source::REMOTE.to_vec(),
            tags: "scenery".into(),
            ratings: vec![Rating::General],
            sort: Sort::Newest,
            cursor,
        };
        let first = combined_search(&state, params(None)).await.unwrap();
        let gelbooru = first.sites.iter().find(|site| site.source == Source::Gelbooru).unwrap();
        assert!(matches!(gelbooru.error, Some(AppError::CredentialsMissing(_))) && !gelbooru.retry);
        let (mut posts, mut next) = (first.posts, first.next);
        for _ in 0..2 {
            let page = combined_search(&state, params(next.clone())).await.unwrap();
            assert!(page.sites.iter().all(|site| site.source == Source::Danbooru && site.error.is_none()));
            posts.extend(page.posts);
            next = page.next;
        }
        println!("3 页共 {} 张，最后的翻页位置 {} 字节", posts.len(), next.as_deref().map_or(0, str::len));
        assert!(posts.len() >= 40, "只有 {} 张", posts.len());
        let times: Vec<i64> = posts.iter().map(|post| combined::rank(post, Sort::Newest).unwrap().0).collect();
        assert!(times.windows(2).all(|pair| pair[0] >= pair[1]));
        // 翻页期间有新图上传时，页码翻页可能重复一两张。
        let ids: HashSet<u64> = posts.iter().map(|post| post.id).collect();
        assert!(ids.len() + 2 >= posts.len());
    }
}
