//! API Key 存在系统钥匙串（macOS 钥匙串 / Windows 凭据管理器），不写进任何文件。
//!
//! 每个站点账号一项：服务名固定，账户名是「站点:用户名」。钥匙串调用可能等待系统弹窗
//! （例如 macOS 询问是否允许访问），调用方要放到阻塞线程里执行。

use keyring::{Entry, Error};

use crate::error::AppError;
use crate::sources::Source;

const SERVICE: &str = "com.ypuddin.imagebox";

fn entry(source: Source, name: &str) -> Result<Entry, AppError> {
    Entry::new(SERVICE, &format!("{}:{name}", source.as_str())).map_err(keychain_error)
}

fn keychain_error(err: Error) -> AppError {
    AppError::Keychain(err.to_string())
}

/// 读取 API Key；钥匙串里没有这一项时返回 `None`。
pub fn read(source: Source, name: &str) -> Result<Option<String>, AppError> {
    match entry(source, name)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(err) => Err(keychain_error(err)),
    }
}

pub fn write(source: Source, name: &str, api_key: &str) -> Result<(), AppError> {
    entry(source, name)?.set_password(api_key).map_err(keychain_error)
}

/// 删除 API Key；本来就没有时也算成功。
pub fn delete(source: Source, name: &str) -> Result<(), AppError> {
    match entry(source, name)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(err) => Err(keychain_error(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实读写系统钥匙串，平时跳过：`cargo test secrets -- --ignored`。
    #[test]
    #[ignore = "会读写系统钥匙串，需要手动运行"]
    fn round_trip_in_system_keychain() {
        let name = format!("__imagebox_test_{}", std::process::id());
        assert_eq!(read(Source::Gelbooru, &name).unwrap(), None);
        write(Source::Gelbooru, &name, "key-1").unwrap();
        write(Source::Gelbooru, &name, "key-2").unwrap();
        assert_eq!(read(Source::Gelbooru, &name).unwrap().as_deref(), Some("key-2"));
        delete(Source::Gelbooru, &name).unwrap();
        delete(Source::Gelbooru, &name).unwrap();
        assert_eq!(read(Source::Gelbooru, &name).unwrap(), None);
    }
}
