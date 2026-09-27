//! 软件设置：存在「软件数据」位置的 settings.json。
//!
//! 只放不敏感的内容：代理、各站点的用户名。API Key 存在系统钥匙串里（见 [`crate::secrets`]），
//! 这里记下用户名，启动时才知道要去钥匙串取哪一项；没填账号的用户完全不会访问钥匙串。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::AppError;
use crate::sources::Source;

pub const FILE_NAME: &str = "settings.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub proxy: ProxySettings,
    #[serde(default)]
    pub accounts: AccountNames,
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
}

impl AccountNames {
    pub fn get(&self, source: Source) -> Option<&SavedAccount> {
        match source {
            Source::Danbooru => self.danbooru.as_ref(),
            Source::Gelbooru => self.gelbooru.as_ref(),
        }
    }

    pub fn set(&mut self, source: Source, account: Option<SavedAccount>) {
        match source {
            Source::Danbooru => self.danbooru = account,
            Source::Gelbooru => self.gelbooru = account,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccount {
    /// Danbooru 是用户名，Gelbooru 是 User ID。
    pub name: String,
    #[serde(default)]
    pub level: Option<String>,
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
        write().map_err(|e| AppError::Internal(format!("保存设置失败：{e}")))
    }
}

/// 检查手动代理地址：支持 http、https、socks5、socks5h；没写协议时按 http 处理
/// （常见的「127.0.0.1:7890」直接可用）。socks 代理没有默认端口，必须写明。
pub fn parse_proxy_url(value: &str) -> Result<Url, AppError> {
    let invalid = |detail: &str| AppError::InvalidInput(format!("代理地址{detail}，例如 http://127.0.0.1:7890"));
    let value = value.trim();
    if value.is_empty() {
        return Err(invalid("不能为空"));
    }
    let with_scheme = if value.contains("://") { value.to_string() } else { format!("http://{value}") };
    let url = Url::parse(&with_scheme).map_err(|_| invalid("格式不对"))?;
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err(AppError::InvalidInput("代理只支持 http、https、socks5 和 socks5h".into()));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(invalid("缺少主机"));
    }
    if url.port_or_known_default().is_none() {
        return Err(invalid("缺少端口"));
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
        assert_eq!(Settings::load(dir.path()).proxy.mode, ProxyMode::None);
    }

    #[test]
    fn saves_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            proxy: ProxySettings { mode: ProxyMode::Manual, url: "socks5://127.0.0.1:1080".into() },
            accounts: AccountNames {
                danbooru: Some(SavedAccount { name: "sora".into(), level: Some("Gold".into()) }),
                gelbooru: None,
            },
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
