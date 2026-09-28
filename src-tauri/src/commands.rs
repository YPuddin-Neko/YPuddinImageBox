use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::error::AppError;
use crate::i18n::{self, text, tr, Language, LanguageSetting};
use crate::library::{
    Folder, GroupPage, GroupQuery, ItemNote, JobInfo, LibraryPage, LibraryQuery, NewSubscription, SavedSearch, Subscription,
};
use crate::settings::{KeyStorage, ProxySettings, SavedAccount};
use crate::sources::filter::{self, QueryPlan};
use crate::sources::{self, danbooru, gelbooru, Page, Post, SearchPage, SearchParams, Source};
use crate::storage::{self, ChangeMode, StorageInfo, StorageKind};
use crate::{keys, net, secrets, thumbs, AppState};

/// 瀑布流每页条数。下载任务另按站点上限（200 / 100）分页。
const PAGE_SIZE: u32 = 40;

/// Danbooru 一次能搜几个 tag：看当前实际登录的账号等级；Gelbooru 不限。
fn tag_limit(state: &AppState, source: Source) -> Option<usize> {
    match source {
        Source::Danbooru => {
            let signed_in = state.accounts.get().danbooru.is_some();
            let level = state.settings().accounts.danbooru.as_ref().and_then(|a| a.level.clone());
            Some(filter::danbooru_tag_limit(if signed_in { level.as_deref() } else { None }))
        }
        Source::Gelbooru => None,
    }
}

/// 超出 tag 上限时一次最多往下翻几页找够一页结果，免得条件太严时一直翻。
const MAX_FILTERED_PAGES: usize = 5;

async fn search_with_plan(state: &AppState, params: &SearchParams, plan: &QueryPlan) -> Result<SearchPage, AppError> {
    let accounts = state.accounts.get();
    let query = &plan.server_query;
    let (posts, next) = if plan.local.is_empty() {
        let page = params.cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(1u32).max(1);
        let (posts, fetched) =
            sources::fetch(&state.net, &accounts, params.source, query, &Page::Number(page), PAGE_SIZE).await?;
        (posts, (fetched >= PAGE_SIZE as usize).then(|| (page + 1).to_string()))
    } else {
        // 按站点每页最多的条数往下翻，本地筛到够一页或翻满几页就先返回，剩下的下次接着翻。
        let mut page = params.cursor.as_deref().and_then(Page::parse).unwrap_or(Page::Number(1));
        let limit = params.source.max_page_size();
        let mut matched = Vec::new();
        let mut next = None;
        for _ in 0..MAX_FILTERED_PAGES {
            let (posts, fetched) = sources::fetch(&state.net, &accounts, params.source, query, &page, limit).await?;
            let ids = posts.iter().map(|post| post.id);
            let Some(bounds) = ids.clone().min().zip(ids.max()).filter(|_| fetched > 0) else {
                next = None;
                break;
            };
            matched.extend(posts.into_iter().filter(|post| plan.local.matches(post)));
            page = page.next(params.source, query, Some(bounds));
            next = Some(page.to_param());
            if matched.len() >= PAGE_SIZE as usize {
                break;
            }
        }
        (matched, next)
    };
    let ids: Vec<u64> = posts.iter().map(|post| post.id).collect();
    let owned = state.library.owned(params.source, &ids).await?.into_iter().collect();
    Ok(SearchPage { posts, next, query: query.clone(), local_filter: plan.local.to_query(), owned })
}

#[tauri::command]
pub async fn search_remote(state: State<'_, AppState>, params: SearchParams) -> Result<SearchPage, AppError> {
    let tags = params.tags_with_sort()?;
    let mut limit = tag_limit(&state, params.source);
    // 站点实际的上限比按账号等级算的小时（例如等级刚变），按站点给的数字重新拆一次。
    for retry in [false, true] {
        let plan = filter::plan_query(params.source, &tags, &params.ratings, limit)?;
        match search_with_plan(&state, &params, &plan).await {
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

/// 查询条件一共能搜到多少张，下载全部结果前给用户确认。
/// Danbooru 的计数接口不限 tag 数量，所以用完整条件，超出上限时也准确。
#[tauri::command]
pub async fn count_remote(state: State<'_, AppState>, params: SearchParams) -> Result<Option<u64>, AppError> {
    let query = sources::build_query(params.source, &params.tags_for_count()?, &params.ratings);
    sources::count(&state.net, &state.accounts.get(), params.source, &query).await
}

#[tauri::command]
pub async fn download_posts(state: State<'_, AppState>, posts: Vec<Post>) -> Result<JobInfo, AppError> {
    state.downloader.enqueue_posts(posts).await
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

/// 收藏当前的搜索条件，返回收藏后的列表。
#[tauri::command]
pub async fn saved_search_add(state: State<'_, AppState>, params: SearchParams) -> Result<Vec<SavedSearch>, AppError> {
    params.sort.term(params.source)?;
    state.library.add_saved_search(params.source, &params.tags, &params.ratings, params.sort).await?;
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
#[tauri::command]
pub async fn account_save(
    state: State<'_, AppState>,
    source: Source,
    name: String,
    api_key: String,
) -> Result<AccountsInfo, AppError> {
    let name = name.trim().to_string();
    let api_key = api_key.trim().to_string();
    if name.is_empty() {
        return Err(AppError::InvalidInput(if source == Source::Danbooru {
            tr!("请填写用户名", "Enter your username")
        } else {
            tr!("请填写 User ID", "Enter your User ID")
        }));
    }
    if api_key.is_empty() {
        return Err(AppError::InvalidInput(tr!("请填写 API Key", "Enter your API key")));
    }
    let level = match source {
        Source::Danbooru => {
            let creds = danbooru::Credentials { username: name.clone(), api_key: api_key.clone() };
            danbooru::verify(&state.net, &creds).await?.level_string
        }
        Source::Gelbooru => {
            let creds = gelbooru::Credentials { user_id: name.clone(), api_key: api_key.clone() };
            gelbooru::verify(&state.net, &creds).await?;
            None
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
    Ok(accounts_info_of(&state))
}

/// 退出登录：删掉保存的 API Key 和设置里的用户名。
#[tauri::command]
pub async fn account_remove(state: State<'_, AppState>, source: Source) -> Result<AccountsInfo, AppError> {
    let saved = state.settings().accounts.get(source).cloned();
    if let Some(saved) = saved {
        blocking(move || keys::forget(source, &saved)).await?;
    }
    state.update_settings(|settings| settings.accounts.set(source, None))?;
    state.accounts.update(|accounts| accounts.set(source, None));
    Ok(accounts_info_of(&state))
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
pub fn proxy_save(state: State<'_, AppState>, proxy: ProxySettings) -> Result<ProxySettings, AppError> {
    proxy.validate()?;
    state.net.apply_proxy(&proxy)?;
    state.update_settings(|settings| settings.proxy = proxy.clone())?;
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
