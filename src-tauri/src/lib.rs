mod commands;
pub mod error;
pub mod net;
mod protocol;
pub mod sources;

use std::sync::Arc;

use tauri::Manager;

use sources::{danbooru, gelbooru};

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
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(AppState { net: Arc::new(net::Net::new()?), credentials: Credentials::from_env() });
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("ibx", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                responder.respond(protocol::serve(&app, &request).await);
            });
        })
        .invoke_handler(tauri::generate_handler![commands::search_remote])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
