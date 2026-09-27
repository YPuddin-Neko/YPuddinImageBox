mod commands;
pub mod error;
pub mod net;
mod protocol;
pub mod sources;
pub mod storage;

use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use tauri::Manager;

use sources::{danbooru, gelbooru};
use storage::{Defaults, Storage};

pub struct Credentials {
    pub danbooru: Option<danbooru::Credentials>,
    pub gelbooru: Option<gelbooru::Credentials>,
}

impl Credentials {
    /// 账号设置（存系统钥匙串）完成之前，先从环境变量读取，方便本地验证：
    /// IMAGEBOX_DANBOORU_USERNAME / IMAGEBOX_DANBOORU_API_KEY，
    /// IMAGEBOX_GELBOORU_USER_ID / IMAGEBOX_GELBOORU_API_KEY。
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        Self {
            danbooru: var("IMAGEBOX_DANBOORU_USERNAME")
                .zip(var("IMAGEBOX_DANBOORU_API_KEY"))
                .map(|(username, api_key)| danbooru::Credentials { username, api_key }),
            gelbooru: var("IMAGEBOX_GELBOORU_USER_ID")
                .zip(var("IMAGEBOX_GELBOORU_API_KEY"))
                .map(|(user_id, api_key)| gelbooru::Credentials { user_id, api_key }),
        }
    }
}

pub struct AppState {
    pub net: Arc<net::Net>,
    pub credentials: Credentials,
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

    pub fn storage_handle(&self) -> Arc<RwLock<Storage>> {
        Arc::clone(&self.storage)
    }
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
            app.manage(AppState {
                net: Arc::new(net::Net::new()?),
                credentials: Credentials::from_env(),
                storage: Arc::new(RwLock::new(storage)),
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
