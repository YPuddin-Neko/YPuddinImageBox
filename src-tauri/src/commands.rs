use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::error::AppError;
use crate::library::{ItemNote, JobInfo, LibraryPage, LibraryQuery};
use crate::sources::{self, Page, Post, SearchPage, SearchParams};
use crate::storage::{self, ChangeMode, StorageInfo, StorageKind};
use crate::AppState;

/// 瀑布流每页条数。下载任务另按站点上限（200 / 100）分页。
const PAGE_SIZE: u32 = 40;

#[tauri::command]
pub async fn search_remote(state: State<'_, AppState>, params: SearchParams) -> Result<SearchPage, AppError> {
    let query = sources::build_query(params.source, &params.tags, &params.ratings);
    let page = params.page.max(1);
    let (posts, fetched) =
        sources::fetch(&state.net, &state.accounts, params.source, &query, &Page::Number(page), PAGE_SIZE).await?;
    let ids: Vec<u64> = posts.iter().map(|post| post.id).collect();
    let owned = state.library.owned(params.source, &ids).await?.into_iter().collect();
    Ok(SearchPage { posts, page, has_more: fetched >= PAGE_SIZE as usize, query, owned })
}

/// 查询条件一共能搜到多少张，下载全部结果前给用户确认。
#[tauri::command]
pub async fn count_remote(state: State<'_, AppState>, params: SearchParams) -> Result<Option<u64>, AppError> {
    let query = sources::build_query(params.source, &params.tags, &params.ratings);
    sources::count(&state.net, &state.accounts, params.source, &query).await
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
    Ok(state.library.list(&query).await?)
}

fn join_error(err: tauri::Error) -> AppError {
    AppError::Internal(err.to_string())
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
