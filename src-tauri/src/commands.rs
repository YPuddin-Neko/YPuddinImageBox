use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::error::AppError;
use crate::library::{ItemNote, JobInfo, LibraryPage, LibraryQuery};
use crate::settings::{ProxySettings, SavedAccount};
use crate::sources::{self, danbooru, gelbooru, Page, Post, SearchPage, SearchParams, Source};
use crate::storage::{self, ChangeMode, StorageInfo, StorageKind};
use crate::{net, secrets, thumbs, AppState};

/// 瀑布流每页条数。下载任务另按站点上限（200 / 100）分页。
const PAGE_SIZE: u32 = 40;

#[tauri::command]
pub async fn search_remote(state: State<'_, AppState>, params: SearchParams) -> Result<SearchPage, AppError> {
    let query = sources::build_query(params.source, &params.tags, &params.ratings);
    let page = params.page.max(1);
    let accounts = state.accounts.get();
    let (posts, fetched) =
        sources::fetch(&state.net, &accounts, params.source, &query, &Page::Number(page), PAGE_SIZE).await?;
    let ids: Vec<u64> = posts.iter().map(|post| post.id).collect();
    let owned = state.library.owned(params.source, &ids).await?.into_iter().collect();
    Ok(SearchPage { posts, page, has_more: fetched >= PAGE_SIZE as usize, query, owned })
}

/// 查询条件一共能搜到多少张，下载全部结果前给用户确认。
#[tauri::command]
pub async fn count_remote(state: State<'_, AppState>, params: SearchParams) -> Result<Option<u64>, AppError> {
    let query = sources::build_query(params.source, &params.tags, &params.ratings);
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
    let query = sources::build_query(params.source, &params.tags, &params.ratings);
    let tags = params.tags.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if tags.is_empty() { "全部帖子".to_string() } else { tags };
    state.downloader.enqueue_query(params.source, &title, &query, max_posts.map(i64::from)).await
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
    /// 启动时读取钥匙串失败的原因。
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
        let field = if source == Source::Danbooru { "用户名" } else { "User ID" };
        return Err(AppError::InvalidInput(format!("请填写{field}")));
    }
    if api_key.is_empty() {
        return Err(AppError::InvalidInput("请填写 API Key".into()));
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

    let previous = state.settings().accounts.get(source).map(|a| a.name.clone());
    let (key_name, key) = (name.clone(), api_key.clone());
    blocking(move || {
        secrets::write(source, &key_name, &key)?;
        // 换了账号时删掉旧账号的 Key，删不掉也不影响新账号使用。
        if let Some(old) = previous.filter(|old| *old != key_name) {
            let _ = secrets::delete(source, &old);
        }
        Ok(())
    })
    .await?;

    state.update_settings(|settings| settings.accounts.set(source, Some(SavedAccount { name: name.clone(), level })))?;
    state.accounts.update(|accounts| match source {
        Source::Danbooru => accounts.danbooru = Some(danbooru::Credentials { username: name, api_key }),
        Source::Gelbooru => accounts.gelbooru = Some(gelbooru::Credentials { user_id: name, api_key }),
    });
    state.clear_accounts_error();
    Ok(accounts_info_of(&state))
}

/// 退出登录：删掉钥匙串里的 API Key 和设置里的用户名。
#[tauri::command]
pub async fn account_remove(state: State<'_, AppState>, source: Source) -> Result<AccountsInfo, AppError> {
    let saved = state.settings().accounts.get(source).map(|a| a.name.clone());
    if let Some(name) = saved {
        blocking(move || secrets::delete(source, &name)).await?;
    }
    state.update_settings(|settings| settings.accounts.set(source, None))?;
    state.accounts.update(|accounts| match source {
        Source::Danbooru => accounts.danbooru = None,
        Source::Gelbooru => accounts.gelbooru = None,
    });
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
