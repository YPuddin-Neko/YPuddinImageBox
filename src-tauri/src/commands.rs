use tauri::State;

use crate::error::AppError;
use crate::sources::{self, danbooru, gelbooru, SearchPage, SearchParams, Source};
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
