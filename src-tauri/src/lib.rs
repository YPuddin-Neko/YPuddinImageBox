mod commands;
pub mod downloader;
pub mod error;
pub mod library;
pub mod net;
mod protocol;
mod secrets;
pub mod settings;
pub mod sources;
pub mod storage;
mod thumbs;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use downloader::{Downloader, Event, EventSink};
use error::AppError;
use library::Library;
use settings::{AccountNames, ProxySettings, Settings};
use sources::{danbooru, gelbooru, AccountStore, Accounts, Source};
use storage::{Defaults, Storage, StorageKind};

pub struct AppState {
    pub net: Arc<net::Net>,
    pub accounts: Arc<AccountStore>,
    pub library: Library,
    pub downloader: Arc<Downloader>,
    /// 见 [`Downloader`] 里的同名字段：移动图片位置时拿写锁。
    pub images_gate: Arc<tokio::sync::RwLock<()>>,
    storage: Arc<RwLock<Storage>>,
    settings: Mutex<Settings>,
    /// 启动时读取钥匙串失败的原因，设置页里提示。
    accounts_error: Mutex<Option<String>>,
}

impl AppState {
    /// 这几把锁只保护内存里的设置，持锁时不会 panic，中毒时直接沿用里面的数据。
    pub fn storage(&self) -> RwLockReadGuard<'_, Storage> {
        self.storage.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn storage_mut(&self) -> RwLockWriteGuard<'_, Storage> {
        self.storage.write().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn settings(&self) -> MutexGuard<'_, Settings> {
        self.settings.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 修改设置并写回「软件数据」位置；写文件失败时内存里的设置保持不变。
    pub fn update_settings(&self, change: impl FnOnce(&mut Settings)) -> Result<(), AppError> {
        let dir = self.storage().path(StorageKind::Data);
        let mut settings = self.settings();
        let mut next = settings.clone();
        change(&mut next);
        next.save(&dir)?;
        *settings = next;
        Ok(())
    }

    pub fn accounts_error(&self) -> Option<String> {
        self.accounts_error.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn clear_accounts_error(&self) {
        *self.accounts_error.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// 按设置里记着的账号去钥匙串取 API Key。没登录的站点不访问钥匙串。
fn load_accounts(names: &AccountNames) -> (Accounts, Option<String>) {
    let mut accounts = Accounts::default();
    let mut error = None;
    for source in [Source::Danbooru, Source::Gelbooru] {
        let Some(saved) = names.get(source) else { continue };
        match secrets::read(source, &saved.name) {
            Ok(Some(api_key)) => match source {
                Source::Danbooru => {
                    accounts.danbooru = Some(danbooru::Credentials { username: saved.name.clone(), api_key })
                }
                Source::Gelbooru => {
                    accounts.gelbooru = Some(gelbooru::Credentials { user_id: saved.name.clone(), api_key })
                }
            },
            Ok(None) => {}
            Err(err) => error = Some(err.to_string()),
        }
    }
    (accounts, error)
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
            let storage = load_storage(app)?;
            let (db_dir, data_dir) = (storage.path(StorageKind::Database), storage.path(StorageKind::Data));
            let storage = Arc::new(RwLock::new(storage));
            let settings = Settings::load(&data_dir);
            // 存下来的代理地址失效时先用系统代理启动，设置页里还能看到原来填的地址。
            let net = net::Net::new(&settings.proxy).or_else(|_| net::Net::new(&ProxySettings::default()))?;
            let net = Arc::new(net);
            let (accounts, accounts_error) = load_accounts(&settings.accounts);
            let accounts = Arc::new(AccountStore::new(accounts));
            let library = tauri::async_runtime::block_on(async {
                let library = Library::open(&db_dir).await?;
                library.requeue_interrupted().await?;
                Ok::<_, sqlx::Error>(library)
            })?;
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
            app.manage(AppState {
                net,
                accounts,
                library,
                downloader,
                images_gate,
                storage,
                settings: Mutex::new(settings),
                accounts_error: Mutex::new(accounts_error),
            });
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
            commands::library_delete,
            commands::accounts_info,
            commands::account_save,
            commands::account_remove,
            commands::proxy_info,
            commands::proxy_save,
            commands::proxy_test,
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
