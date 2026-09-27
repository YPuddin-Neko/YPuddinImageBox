mod commands;
pub mod downloader;
pub mod error;
mod keys;
pub mod library;
pub mod net;
mod protocol;
mod sealed;
mod secrets;
pub mod settings;
pub mod sources;
pub mod storage;
mod thumbs;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(target_os = "macos")]
use tauri::RunEvent;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use downloader::{Downloader, Event, EventSink};
use error::AppError;
use library::Library;
use settings::{ProxySettings, Settings};
use sources::{AccountStore, Source};
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

const MAIN_WINDOW: &str = "main";
/// 开机自动启动时带上这个参数：启动后不弹出窗口，直接在后台运行。
const BACKGROUND_ARG: &str = "--background";
/// 下次打开时恢复窗口的大小、位置和是否最大化。显示与否由启动方式决定，不恢复。
const WINDOW_STATE: StateFlags = StateFlags::SIZE.union(StateFlags::POSITION).union(StateFlags::MAXIMIZED);
/// 日志文件放在「软件数据」位置的 logs 目录，超过 2 MB 换新文件，保留上一份。
const LOG_FILE: &str = "imagebox";
const LOG_MAX_BYTES: u128 = 2 * 1024 * 1024;

/// 日志文件的完整路径，设置页里用来在访达 / 资源管理器中显示。
pub fn log_file(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("logs").join(format!("{LOG_FILE}.log"))
}

/// 写日志到文件和终端，并把崩溃信息也记进日志。
fn setup_logging(app: &tauri::App, data_dir: &std::path::Path) -> tauri::Result<()> {
    let folder = TargetKind::Folder { path: data_dir.join("logs"), file_name: Some(LOG_FILE.into()) };
    app.handle().plugin(
        tauri_plugin_log::Builder::new()
            .clear_targets()
            .targets([Target::new(TargetKind::Stdout), Target::new(folder)])
            .level(log::LevelFilter::Info)
            // 数据库每条语句都会记一行，只留警告。
            .level_for("sqlx", log::LevelFilter::Warn)
            .max_file_size(LOG_MAX_BYTES)
            .rotation_strategy(RotationStrategy::KeepOne)
            .timezone_strategy(TimezoneStrategy::UseLocal)
            .build(),
    )?;
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("程序崩溃：{info}");
        default_hook(info);
    }));
    log::info!(
        "ImageBox {} 启动（{} {}）",
        app.package_info().version,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    Ok(())
}

fn notify(app: &AppHandle, body: &str) {
    let _ = app.notification().builder().title("ImageBox").body(body).show();
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 菜单栏（Windows 上是托盘）图标：打开窗口、立即检查订阅、退出。
/// macOS 上点图标弹菜单；Windows 上左键打开窗口、右键弹菜单。
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开 ImageBox", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "立即检查订阅", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 ImageBox", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &check, &separator, &quit])?;
    let tray = TrayIconBuilder::with_id("main")
        .tooltip("ImageBox")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "check" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let started = app.state::<AppState>().downloader.check_all_subscriptions().await;
                    if let Ok(0) = started {
                        notify(&app, "订阅都在检查中，或者还没有启用的订阅");
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if !cfg!(target_os = "macos") {
                    show_main_window(tray.app_handle());
                }
            }
        });
    let tray = if cfg!(target_os = "macos") {
        // 单色模板图，系统按菜单栏的深浅色自动着色。
        tray.icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?).icon_as_template(true)
    } else {
        match app.default_window_icon() {
            Some(icon) => tray.icon(icon.clone()),
            None => tray,
        }
    };
    tray.build(app)?;
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedPayload {
    source: Source,
    post_id: u64,
}

/// 下载队列的变化推给界面：job-updated / job-removed / library-changed / subscription-updated；
/// 订阅下载到新图时发系统通知。
fn emit_event(app: &AppHandle, event: Event) {
    let result = match event {
        Event::Job(job) => app.emit("job-updated", job),
        Event::JobRemoved(id) => app.emit("job-removed", id),
        Event::Saved { source, post_id } => app.emit("library-changed", SavedPayload { source, post_id }),
        Event::Subscription(sub) => app.emit("subscription-updated", sub),
        Event::NewPosts { title, saved } => {
            notify(app, &format!("订阅「{title}」下载了 {saved} 张新图"));
            Ok(())
        }
    };
    if let Err(err) = result {
        log::warn!("发送界面事件失败：{err}");
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
    let app = tauri::Builder::default()
        // 软件已经在运行（例如开机启动后又手动打开）时，只把已有的窗口叫出来，不再开第二份。必须最先注册。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_window_state::Builder::new().with_state_flags(WINDOW_STATE).build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::Builder::new().args([BACKGROUND_ARG]).build())
        .setup(|app| {
            let storage = load_storage(app)?;
            let (db_dir, data_dir) = (storage.path(StorageKind::Database), storage.path(StorageKind::Data));
            setup_logging(app, &data_dir)?;
            let storage = Arc::new(RwLock::new(storage));
            let settings = Settings::load(&data_dir);
            // 存下来的代理地址失效时先用系统代理启动，设置页里还能看到原来填的地址。
            let net = net::Net::new(&settings.proxy).or_else(|_| net::Net::new(&ProxySettings::default()))?;
            let net = Arc::new(net);
            let (accounts, accounts_error) = keys::load(&settings);
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
            tauri::async_runtime::spawn(Arc::clone(&downloader).run_schedule());
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
            setup_tray(app)?;
            // 窗口在配置里默认隐藏，开机自动启动时保持隐藏，其余情况显示出来，避免先闪一下再藏起来。
            if !std::env::args().any(|arg| arg == BACKGROUND_ARG) {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 设置里选了「后台继续运行」时，关窗口只是藏起来，订阅和下载照常进行。
            if let WindowEvent::CloseRequested { api, .. } = event {
                let keep_running = window.app_handle().state::<AppState>().settings().close_to_tray;
                if keep_running && window.label() == MAIN_WINDOW {
                    api.prevent_close();
                    // 藏起来之后软件可能很久不退出，窗口位置现在就存下来。
                    let _ = window.app_handle().save_window_state(WINDOW_STATE);
                    let _ = window.hide();
                }
            }
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
            commands::account_key_storage,
            commands::subscriptions_list,
            commands::subscription_preview,
            commands::subscription_create,
            commands::subscription_update,
            commands::subscription_delete,
            commands::subscription_check,
            commands::subscriptions_check_all,
            commands::general_info,
            commands::general_save,
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
        .build(tauri::generate_context!())
        .expect("error while running tauri application");
    app.run(|app, event| {
        // macOS：窗口藏起来后点程序坞图标，重新显示窗口。这个事件只有 macOS 有。
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            show_main_window(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}
