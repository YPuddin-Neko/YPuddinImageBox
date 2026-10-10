//! 软件设置：存在「软件数据」位置的 settings.json。
//!
//! 代理、各站点的用户名，以及 API Key 的保存方式。API Key 默认存在系统钥匙串里（见 [`crate::secrets`]），
//! 这里只记用户名；用户选了「加密保存在设置文件」时，这里存的是密文（见 [`crate::sealed`]）。
//! 没填账号的用户完全不会访问钥匙串。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::AppError;
use crate::i18n::{tr, LanguageSetting};
use crate::sources::Source;

pub const FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub proxy: ProxySettings,
    #[serde(default)]
    pub accounts: AccountNames,
    /// 新保存的 API Key 放在哪。
    #[serde(default)]
    pub key_storage: KeyStorage,
    /// 加密保存 API Key 用的随机盐，第一次需要时生成。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_salt: Option<String>,
    /// 关闭窗口后在后台继续运行（订阅检查和下载不中断），从菜单栏 / 托盘图标重新打开。
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// 界面语言。
    #[serde(default)]
    pub language: LanguageSetting,
    #[serde(default)]
    pub fanbox_download: FanboxDownloadSettings,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            proxy: ProxySettings::default(),
            accounts: AccountNames::default(),
            key_storage: KeyStorage::default(),
            key_salt: None,
            close_to_tray: true,
            language: LanguageSetting::default(),
            fanbox_download: FanboxDownloadSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FanboxDownloadSettings {
    pub directory: Option<PathBuf>,
    pub folder_template: String,
    pub image_template: String,
    pub attachment_template: String,
}

impl Default for FanboxDownloadSettings {
    fn default() -> Self {
        Self {
            directory: None,
            folder_template: "{user}/{date}-{title}".into(),
            image_template: "{index}".into(),
            attachment_template: "{name}".into(),
        }
    }
}

impl FanboxDownloadSettings {
    pub fn normalized_for_storage(self, storage: &crate::storage::Storage) -> Result<Self, AppError> {
        let mut settings = self.normalized()?;
        let default = storage.path(crate::storage::StorageKind::Images).join("fanbox");
        if settings.directory.as_ref().is_some_and(|path| crate::storage::normalize(path) == crate::storage::normalize(&default)) {
            settings.directory = None;
        }
        settings.validate_storage(storage)?;
        Ok(settings)
    }

    pub fn validate_storage(&self, storage: &crate::storage::Storage) -> Result<(), AppError> {
        self.validate()?;
        let images = storage.path(crate::storage::StorageKind::Images);
        for cache in storage.locations(crate::storage::StorageKind::Cache) {
            self.validate_cache_path(&images, &cache)?;
        }
        Ok(())
    }

    pub fn validate_cache_path(&self, images: &Path, cache: &Path) -> Result<(), AppError> {
        let directory = self.directory.as_ref().filter(|path| !path.to_string_lossy().trim().is_empty())
            .cloned().unwrap_or_else(|| images.join("fanbox"));
        if crate::storage::paths_overlap(&directory, cache) {
            return Err(AppError::InvalidInput(tr!(
                "FANBOX 保存位置不能与缓存位置重叠",
                "The FANBOX save location must not overlap the cache location"
            )));
        }
        Ok(())
    }

    /// 旧版本可能把默认位置存成固定路径；图片位置改变时恢复跟随，随图片移动的自定义子目录一起改写。
    pub fn after_images_change(&self, from: &Path, to: &Path, moving: bool) -> Self {
        let mut settings = self.clone();
        if let Some(directory) = &self.directory {
            let default = crate::storage::normalize(&from.join("fanbox"));
            let directory = crate::storage::normalize(directory);
            let from = crate::storage::normalize(from);
            if directory == default {
                settings.directory = None;
            } else if moving {
                if let Ok(relative) = directory.strip_prefix(&from) {
                    settings.directory = Some(to.join(relative));
                }
            }
        }
        settings
    }

    pub fn normalized(mut self) -> Result<Self, AppError> {
        if self.directory.as_ref().is_some_and(|path| path.to_string_lossy().trim().is_empty()) {
            self.directory = None;
        }
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if let Some(path) = self.directory.as_ref().filter(|path| !path.to_string_lossy().trim().is_empty()) {
            if !path.is_absolute() {
                return Err(AppError::InvalidInput(tr!(
                    "FANBOX 下载目录必须是绝对路径",
                    "The FANBOX download directory must be an absolute path"
                )));
            }
            if path.is_file() {
                return Err(AppError::InvalidInput(tr!(
                    "FANBOX 下载目录指向了文件，请选择文件夹",
                    "The FANBOX download directory points to a file; choose a folder"
                )));
            }
        }
        validate_fanbox_template(&self.folder_template, true)?;
        validate_fanbox_template(&self.image_template, false)?;
        validate_fanbox_template(&self.attachment_template, false)
    }
}

fn validate_fanbox_template(value: &str, folder: bool) -> Result<(), AppError> {
    if value.trim().is_empty() || value.chars().count() > 500 {
        return Err(AppError::InvalidInput(tr!(
            "FANBOX 命名模板不能为空，且不能超过 500 个字符",
            "FANBOX naming templates must contain 1 to 500 characters"
        )));
    }
    let parts: Vec<&str> = value.split('/').collect();
    let has_drive = |part: &&str| {
        let bytes = part.trim().as_bytes();
        bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
    };
    if value.contains('\\')
        || value.chars().any(char::is_control)
        || parts.iter().any(|part| matches!(part.trim(), "" | "." | ".."))
        || parts.iter().any(has_drive)
        || parts.len() > if folder { 10 } else { 1 }
    {
        return Err(AppError::InvalidInput(if folder {
            tr!(
                "FANBOX 文件夹模板须为相对路径，最多 10 层；用 / 分隔，不能包含空层级、.、..、反斜杠或盘符",
                "The FANBOX folder template must be a relative path of at most 10 levels, separated by /; empty levels, ., .., backslashes and drive letters are not allowed"
            )
        } else {
            tr!(
                "FANBOX 文件名模板不能使用路径分隔符、盘符或单独的 .、..",
                "FANBOX filename templates cannot use path separators, drive letters, or a standalone . or .."
            )
        }));
    }
    let mut rest = value;
    while let Some(start) = rest.find(['{', '}']) {
        let token = &rest[start..];
        let Some(end) = token.find('}').filter(|end| *end > 0 && token.starts_with('{')) else {
            return Err(AppError::InvalidInput(tr!(
                "FANBOX 命名模板的占位符括号不完整",
                "A placeholder in the FANBOX naming template has unmatched braces"
            )));
        };
        let name = &token[1..end];
        if !matches!(name, "user" | "creator_id" | "date" | "title" | "postid" | "index" | "name") {
            return Err(AppError::InvalidInput(tr!(
                "FANBOX 命名模板不支持占位符：{name}",
                "Unsupported FANBOX naming placeholder: {name}"
            )));
        }
        rest = &token[end + 1..];
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyStorage {
    /// 系统钥匙串（macOS 钥匙串 / Windows 凭据管理器）。
    #[default]
    Keychain,
    /// 加密后存在设置文件里。
    File,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// 使用系统设置里的代理。
    #[default]
    System,
    /// 直接连接。
    None,
    /// 使用下面填写的代理地址。
    Manual,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    #[serde(default)]
    pub mode: ProxyMode,
    /// 手动代理地址；切换到其他模式时保留，方便切回来。
    #[serde(default)]
    pub url: String,
}

/// 已登录账号的用户名和等级（等级只用于显示）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountNames {
    #[serde(default)]
    pub danbooru: Option<SavedAccount>,
    #[serde(default)]
    pub gelbooru: Option<SavedAccount>,
    #[serde(default)]
    pub e621: Option<SavedAccount>,
    #[serde(default)]
    pub rule34: Option<SavedAccount>,
    /// Pixiv 的名字是账号昵称，「Key」是登录后的 PHPSESSID。
    #[serde(default)]
    pub pixiv: Option<SavedAccount>,
    /// FANBOX 的名字是账号昵称，「Key」是登录后的 FANBOXSESSID。
    #[serde(default)]
    pub fanbox: Option<SavedAccount>,
    /// Kemono 的名字是用户名，「Key」是登录后的 session Cookie。
    #[serde(default)]
    pub kemono: Option<SavedAccount>,
    /// Yande.re 只存用户名，用来列出收藏；没有 Key。
    #[serde(default)]
    pub yandere: Option<SavedAccount>,
}

impl AccountNames {
    pub fn get(&self, source: Source) -> Option<&SavedAccount> {
        match source {
            Source::Danbooru => self.danbooru.as_ref(),
            Source::Gelbooru => self.gelbooru.as_ref(),
            Source::E621 => self.e621.as_ref(),
            Source::Rule34 => self.rule34.as_ref(),
            Source::Pixiv => self.pixiv.as_ref(),
            Source::Fanbox => self.fanbox.as_ref(),
            Source::Kemono => self.kemono.as_ref(),
            Source::Yandere => self.yandere.as_ref(),
            Source::X => None,
            Source::Custom => None,
        }
    }

    pub fn get_mut(&mut self, source: Source) -> Option<&mut SavedAccount> {
        match source {
            Source::Danbooru => self.danbooru.as_mut(),
            Source::Gelbooru => self.gelbooru.as_mut(),
            Source::E621 => self.e621.as_mut(),
            Source::Rule34 => self.rule34.as_mut(),
            Source::Pixiv => self.pixiv.as_mut(),
            Source::Fanbox => self.fanbox.as_mut(),
            Source::Kemono => self.kemono.as_mut(),
            Source::Yandere => self.yandere.as_mut(),
            Source::X => None,
            Source::Custom => None,
        }
    }

    pub fn set(&mut self, source: Source, account: Option<SavedAccount>) {
        match source {
            Source::Danbooru => self.danbooru = account,
            Source::Gelbooru => self.gelbooru = account,
            Source::E621 => self.e621 = account,
            Source::Rule34 => self.rule34 = account,
            Source::Pixiv => self.pixiv = account,
            Source::Fanbox => self.fanbox = account,
            Source::Kemono => self.kemono = account,
            Source::Yandere => self.yandere = account,
            // 不用登录的站点没有账号可存。
            Source::X => {}
            Source::Custom => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccount {
    /// Danbooru/e621 是用户名，Gelbooru/Rule34.xxx 是 User ID。
    pub name: String,
    #[serde(default)]
    pub level: Option<String>,
    /// 加密后的 API Key；为空表示存在系统钥匙串里。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_key: Option<String>,
}

impl Settings {
    /// 读取设置；文件不存在或内容损坏时用默认值。
    pub fn load(dir: &Path) -> Self {
        fs::read(dir.join(FILE_NAME))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<(), AppError> {
        let write = || -> io::Result<()> {
            fs::create_dir_all(dir)?;
            let file: PathBuf = dir.join(FILE_NAME);
            let temp = file.with_extension("json.part");
            fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
            fs::rename(&temp, &file)
        };
        write().map_err(|e| AppError::Internal(tr!("保存设置失败：{e}", "Couldn't save the settings: {e}")))
    }
}

/// 检查手动代理地址：支持 http、https、socks5、socks5h；没写协议时按 http 处理
/// （常见的「127.0.0.1:7890」直接可用）。socks 代理没有默认端口，必须写明。
pub fn parse_proxy_url(value: &str) -> Result<Url, AppError> {
    const EXAMPLE: &str = "http://127.0.0.1:7890";
    let invalid = |zh: &str, en: &str| AppError::InvalidInput(tr!("代理地址{zh}，例如 {EXAMPLE}", "{en}, e.g. {EXAMPLE}"));
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid("不能为空", "Enter a proxy address"));
    }
    let with_scheme = if value.contains("://") { value.to_string() } else { format!("http://{value}") };
    let url = Url::parse(&with_scheme).map_err(|_| invalid("格式不对", "The proxy address isn't valid"))?;
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err(AppError::InvalidInput(tr!(
            "代理只支持 http、https、socks5 和 socks5h",
            "Only http, https, socks5 and socks5h proxies are supported"
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(invalid("缺少主机", "The proxy address is missing a host"));
    }
    if url.port_or_known_default().is_none() {
        return Err(invalid("缺少端口", "The proxy address is missing a port"));
    }
    Ok(url)
}

impl ProxySettings {
    /// 保存前的检查：手动模式下地址必须有效。
    pub fn validate(&self) -> Result<(), AppError> {
        if self.mode == ProxyMode::Manual {
            parse_proxy_url(&self.url)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_broken_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        fs::write(dir.path().join(FILE_NAME), b"{not json").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        // 旧版本写出的文件缺字段时补默认值。
        fs::write(dir.path().join(FILE_NAME), br#"{"proxy":{"mode":"none"}}"#).unwrap();
        let loaded = Settings::load(dir.path());
        assert_eq!(loaded.proxy.mode, ProxyMode::None);
        assert!(loaded.close_to_tray);
        assert_eq!(loaded.fanbox_download, FanboxDownloadSettings::default());
    }

    #[test]
    fn saves_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            proxy: ProxySettings { mode: ProxyMode::Manual, url: "socks5://127.0.0.1:1080".into() },
            accounts: AccountNames {
                danbooru: Some(SavedAccount { name: "sora".into(), level: Some("Gold".into()), sealed_key: None }),
                gelbooru: Some(SavedAccount { name: "42".into(), level: None, sealed_key: Some("v1.abc".into()) }),
                e621: None,
                rule34: None,
                pixiv: None,
                fanbox: Some(SavedAccount { name: "creator".into(), level: None, sealed_key: Some("v1.fanbox".into()) }),
                kemono: None,
                yandere: Some(SavedAccount { name: "yuki".into(), level: None, sealed_key: None }),
            },
            key_storage: KeyStorage::File,
            key_salt: Some("salt".into()),
            close_to_tray: false,
            language: LanguageSetting::En,
            fanbox_download: FanboxDownloadSettings {
                directory: Some(dir.path().join("FANBOX archive")),
                folder_template: "{creator_id}/{date}-{postid}-{title}".into(),
                image_template: "作品_{index}".into(),
                attachment_template: "{index}_{name}".into(),
            },
        };
        settings.save(&dir.path().join("nested")).unwrap();
        assert_eq!(Settings::load(&dir.path().join("nested")), settings);
    }

    #[test]
    fn fanbox_templates_accept_supported_tokens_and_keep_unicode() {
        let settings = FanboxDownloadSettings {
            directory: Some(PathBuf::from("   ")),
            folder_template: "作者/{user}/{creator_id}/{date}-{title}-{postid}".into(),
            image_template: "原图 {index}·{name}".into(),
            attachment_template: "附件 {postid}-{index}-{name}".into(),
        };
        let normalized = settings.clone().normalized().unwrap();
        assert!(normalized.directory.is_none());
        assert_eq!(normalized.folder_template, settings.folder_template);
        assert_eq!(normalized.image_template, settings.image_template);
        assert_eq!(normalized.attachment_template, settings.attachment_template);
        let restored: FanboxDownloadSettings = serde_json::from_str(r#"{"imageTemplate":"{name}"}"#).unwrap();
        assert_eq!(restored.folder_template, FanboxDownloadSettings::default().folder_template);
        assert_eq!(restored.image_template, "{name}");
        assert_eq!(restored.attachment_template, "{name}");
        assert!(restored.directory.is_none());
    }

    #[test]
    fn fanbox_templates_reject_traversal_absolute_paths_and_unknown_tokens() {
        for value in [
            "", " ", "/{user}", "{user}/", "{user}//{title}", "../{user}", "{user}/./{title}",
            "{user}/.. /{title}", "{user}\\{title}", "C:/{user}", " C:{user}", "a/C:relative", "a\n{title}",
            "{unknown}", "{user", "user}", "{{user}}", "{}", "{user}/{title}}", "{title}{",
        ] {
            let settings = FanboxDownloadSettings { folder_template: value.into(), ..FanboxDownloadSettings::default() };
            assert!(settings.validate().is_err(), "{value}");
        }
        for value in ["../{name}", "{name}/child", "{name}\\child", ".", "..", "C:{name}", "{unknown}", "{name"] {
            let image = FanboxDownloadSettings { image_template: value.into(), ..FanboxDownloadSettings::default() };
            let attachment = FanboxDownloadSettings { attachment_template: value.into(), ..FanboxDownloadSettings::default() };
            assert!(image.validate().is_err(), "{value}");
            assert!(attachment.validate().is_err(), "{value}");
        }
        for (levels, valid) in [(10, true), (11, false)] {
            let settings = FanboxDownloadSettings { folder_template: vec!["作品"; levels].join("/"), ..FanboxDownloadSettings::default() };
            assert_eq!(settings.validate().is_ok(), valid);
        }
        for (length, valid) in [(500, true), (501, false)] {
            let settings = FanboxDownloadSettings { folder_template: "画".repeat(length), ..FanboxDownloadSettings::default() };
            assert_eq!(settings.validate().is_ok(), valid);
        }
    }

    #[test]
    fn fanbox_directory_validation_does_not_create_folders() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("new archive");
        let settings = FanboxDownloadSettings { directory: Some(destination.clone()), ..FanboxDownloadSettings::default() };
        assert!(settings.validate().is_ok());
        assert!(!destination.exists());
        std::fs::write(&destination, b"existing file").unwrap();
        assert!(settings.validate().is_err());
        let relative = FanboxDownloadSettings { directory: Some(PathBuf::from("relative/archive")), ..settings };
        assert!(relative.validate().is_err());
    }

    #[test]
    fn older_accounts_keep_their_names_without_fanbox() {
        let accounts: AccountNames = serde_json::from_str(r#"{"pixiv":{"name":"existing","sealedKey":"v1.pixiv"}}"#).unwrap();
        assert_eq!(accounts.pixiv.as_ref().unwrap().name, "existing");
        assert_eq!(accounts.pixiv.as_ref().unwrap().sealed_key.as_deref(), Some("v1.pixiv"));
        assert!(accounts.fanbox.is_none());
    }

    #[test]
    fn proxy_urls_need_known_scheme_host_and_port() {
        assert!(parse_proxy_url("http://127.0.0.1:7890").is_ok());
        assert!(parse_proxy_url(" socks5h://user:pw@proxy.lan:1080 ").is_ok());
        // 没写协议按 http。
        assert_eq!(parse_proxy_url("127.0.0.1:7890").unwrap().as_str(), "http://127.0.0.1:7890/");
        assert!(parse_proxy_url("").is_err());
        assert!(parse_proxy_url("ftp://127.0.0.1:21").is_err());
        assert!(parse_proxy_url("socks5://127.0.0.1").is_err());
        assert!(parse_proxy_url("http://").is_err());
        let manual = ProxySettings { mode: ProxyMode::Manual, url: "garbage url".into() };
        assert!(manual.validate().is_err());
        let off = ProxySettings { mode: ProxyMode::System, url: "garbage url".into() };
        assert!(off.validate().is_ok());
    }
}
