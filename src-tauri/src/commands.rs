use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::AppError;
use crate::sources::{self, danbooru, gelbooru, SearchPage, SearchParams, Source};
use crate::storage::{self, ChangeMode, StorageInfo, StorageKind};
use crate::AppState;

/// 瀑布流每页条数。下载任务另按站点上限（200 / 100）分页。
const PAGE_SIZE: u32 = 40;

#[tauri::command]
pub async fn search_remote(state: State<'_, AppState>, params: SearchParams) -> Result<SearchPage, AppError> {
    let query = sources::build_query(params.source, &params.tags, &params.ratings);
    let page = params.page.max(1);
    let (posts, fetched) = match params.source {
        Source::Danbooru => {
            danbooru::search(&state.net, &query, page, PAGE_SIZE, state.credentials.danbooru.as_ref()).await?
        }
        Source::Gelbooru => {
            let creds = state.credentials.gelbooru.as_ref().ok_or(AppError::CredentialsMissing("Gelbooru"))?;
            gelbooru::search(&state.net, &query, page, PAGE_SIZE, creds).await?
        }
    };
    Ok(SearchPage { posts, page, has_more: fetched >= PAGE_SIZE as usize, query })
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
    let plan = state.storage().plan(kind, path.map(PathBuf::from), mode)?;
    // 移动在锁外进行：搬大量图片时，图片协议仍能照常读取当前位置。
    let plan = if kind.applies_on_restart() {
        plan
    } else {
        tauri::async_runtime::spawn_blocking(move || plan.execute().map(|_| plan)).await.map_err(join_error)??
    };
    let mut storage = state.storage_mut();
    let applied = storage.commit(&plan)?;
    Ok(ChangeOutcome { applied, info: storage.info() })
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
