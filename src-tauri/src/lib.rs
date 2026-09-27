mod commands;
pub mod downloader;
pub mod error;
pub mod library;
pub mod net;
mod protocol;
pub mod sources;
pub mod storage;
mod thumbs;

use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use downloader::{Downloader, Event, EventSink};
use library::Library;
use sources::{Accounts, Source};
use storage::{Defaults, Storage, StorageKind};

pub struct AppState {
    pub net: Arc<net::Net>,
    pub accounts: Arc<Accounts>,
    pub library: Library,
    pub downloader: Arc<Downloader>,
    /// 见 [`Downloader`] 里的同名字段：移动图片位置时拿写锁。
    pub images_gate: Arc<tokio::sync::RwLock<()>>,
    storage: Arc<RwLock<Storage>>,
}

impl AppState {
    /// 锁只保护路径设置，持锁时不会 panic，中毒时直接沿用里面的数据。
    pub fn storage(&self) -> RwLockReadGuard<'_, Storage> {
        self.storage.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn storage_mut(&self) -> RwLockWriteGuard<'_, Storage> {
        self.storage.write().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedPayload {
    source: Source,
    post_id: u64,
}

/// 下载队列的变化推给界面：job-updated / job-removed / library-changed。
fn emit_event(app: &AppHandle, event: Event) {
    let result = match event {
        Event::Job(job) => app.emit("job-updated", job),
        Event::JobRemoved(id) => app.emit("job-removed", id),
        Event::Saved { source, post_id } => app.emit("library-changed", SavedPayload { source, post_id }),
    };
    #[cfg(debug_assertions)]
    if let Err(err) = result {
        eprintln!("[event] 发送失败：{err}");
    }
    #[cfg(not(debug_assertions))]
    let _ = result;
}

fn load_storage(app: &tauri::App) -> Result<Storage, Box<dyn std::error::Error>> {
    let paths = app.path();
    let defaults = Defaults {
        images: paths.picture_dir().or_else(|_| paths.home_dir())?.join("ImageBox"),
        // 放在子目录里：macOS 上配置目录和数据目录是同一个，storage.json 不能被当成软件数据一起移走。
        data: paths.app_data_dir()?.join("data"),
        // 系统 WebView 也把缓存放在应用缓存目录里，这里用单独的子目录，移动或清空时不碰它。
        cache: paths.app_cache_dir()?.join("image-cache"),
    };
    let mut storage = Storage::load(paths.app_config_dir()?.join("storage.json"), defaults);
    storage.apply_pending();
    storage.ensure_runtime_dirs()?;
    Ok(storage)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let storage = Arc::new(RwLock::new(load_storage(app)?));
            let db_dir = storage.read().unwrap_or_else(PoisonError::into_inner).path(StorageKind::Database);
            let library = tauri::async_runtime::block_on(async {
                let library = Library::open(&db_dir).await?;
                library.requeue_interrupted().await?;
                Ok::<_, sqlx::Error>(library)
            })?;
            let net = Arc::new(net::Net::new()?);
            let accounts = Arc::new(Accounts::from_env());
            let images_gate = Arc::new(tokio::sync::RwLock::new(()));
            let handle = app.handle().clone();
            let events: EventSink = Arc::new(move |event| emit_event(&handle, event));
            let downloader = Downloader::new(
                library.clone(),
                Arc::clone(&net),
                Arc::clone(&accounts),
                Arc::clone(&storage),
                Arc::clone(&images_gate),
                events,
            );
            tauri::async_runtime::spawn(Arc::clone(&downloader).run());
            app.manage(AppState { net, accounts, library, downloader, images_gate, storage });
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("ibx", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                responder.respond(protocol::serve(&app, &request).await);
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::search_remote,
            commands::count_remote,
            commands::download_posts,
            commands::download_query,
            commands::list_jobs,
            commands::job_action,
            commands::job_notes,
            commands::clear_finished_jobs,
            commands::library_list,
            commands::storage_info,
            commands::storage_usage,
            commands::storage_change,
            commands::storage_cancel_pending,
            commands::storage_dismiss_error,
            commands::storage_prepare,
            commands::restart_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
