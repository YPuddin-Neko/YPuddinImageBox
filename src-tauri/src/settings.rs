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
        }
    }
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
}

impl AccountNames {
    pub fn get(&self, source: Source) -> Option<&SavedAccount> {
        match source {
            Source::Danbooru => self.danbooru.as_ref(),
            Source::Gelbooru => self.gelbooru.as_ref(),
            Source::E621 => self.e621.as_ref(),
            Source::Rule34 => self.rule34.as_ref(),
            Source::Pixiv => self.pixiv.as_ref(),
            Source::Kemono => None,
            Source::Yandere => None,
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
            Source::Kemono => None,
            Source::Yandere => None,
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
            Source::Kemono => {}
            // 不用登录的站点没有账号可存。
            Source::Yandere => {}
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
            },
            key_storage: KeyStorage::File,
            key_salt: Some("salt".into()),
            close_to_tray: false,
            language: LanguageSetting::En,
        };
        settings.save(&dir.path().join("nested")).unwrap();
        assert_eq!(Settings::load(&dir.path().join("nested")), settings);
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
