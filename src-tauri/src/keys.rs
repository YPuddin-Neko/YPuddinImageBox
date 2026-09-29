//! API Key 按用户选的方式保存：系统钥匙串，或加密后存在设置文件里（见 [`crate::sealed`]）。
//!
//! 读取时看每个账号自己存在哪：设置里有密文就解密，没有就去钥匙串取，所以切换方式时不会漏掉。
//! 钥匙串调用可能等待系统弹窗，这里的函数都要在阻塞线程里调用。

use crate::error::AppError;
use crate::i18n::{text, tr};
use crate::settings::{AccountNames, KeyStorage, SavedAccount, Settings};
use crate::sources::{Accounts, Source};
use crate::{sealed, secrets};

/// 加密时绑定的附加数据，密文不能挪给别的账号用。
fn context(source: Source, name: &str) -> String {
    format!("{}:{name}", source.as_str())
}

fn read(settings: &Settings, source: Source, saved: &SavedAccount) -> Result<Option<String>, AppError> {
    match (&saved.sealed_key, &settings.key_salt) {
        (Some(sealed), Some(salt)) => sealed::open(sealed, salt, &context(source, &saved.name)).map(Some),
        (Some(_), None) => Err(AppError::Internal(tr!(
            "设置文件里缺少加密盐，请重新填写 API Key",
            "The settings file is missing its encryption salt. Enter the API key again"
        ))),
        (None, _) => secrets::read(source, &saved.name),
    }
}

/// 启动时取回已保存账号的 API Key；只访问确实存了账号的地方。第二个值是失败原因。
pub fn load(settings: &Settings) -> (Accounts, Option<String>) {
    let mut accounts = Accounts::default();
    let mut errors = Vec::new();
    for source in Source::ALL {
        let Some(saved) = settings.accounts.get(source) else { continue };
        match read(settings, source, saved) {
            Ok(Some(key)) => accounts.set(source, Some((saved.name.clone(), key))),
            Ok(None) => {}
            Err(err) => {
                let site = source.site_name();
                errors.push(tr!("{site}：{err}", "{site}: {err}"));
            }
        }
    }
    (accounts, (!errors.is_empty()).then(|| errors.join(text("；", "; "))))
}

/// 按当前方式保存一个账号的 Key，返回写进设置的密文（钥匙串方式为 `None`）。
/// 加密方式第一次用时会在 `settings` 里生成盐，调用方要把它一起保存。
pub fn store(settings: &mut Settings, source: Source, name: &str, api_key: &str) -> Result<Option<String>, AppError> {
    match settings.key_storage {
        KeyStorage::Keychain => {
            secrets::write(source, name, api_key)?;
            Ok(None)
        }
        KeyStorage::File => {
            let salt = match &settings.key_salt {
                Some(salt) => salt.clone(),
                None => settings.key_salt.insert(sealed::new_salt()?).clone(),
            };
            sealed::seal(api_key, &salt, &context(source, name)).map(Some)
        }
    }
}

/// 删掉账号存在钥匙串里的 Key；存在设置文件里的随设置一起删，不用处理。
pub fn forget(source: Source, saved: &SavedAccount) -> Result<(), AppError> {
    if saved.sealed_key.is_none() {
        secrets::delete(source, &saved.name)?;
    }
    Ok(())
}

/// 切换保存方式后的设置，以及切换完成后要从钥匙串里删掉的旧项。
pub struct Migration {
    pub accounts: AccountNames,
    pub key_salt: Option<String>,
    pub stale: Vec<(Source, String)>,
}

/// 把内存里已有的 Key 按新方式重新保存。先写新位置，旧的钥匙串项由调用方在设置保存成功后删除。
/// 找不到 Key 的账号（需要重新填写的）保持原样。
pub fn migrate(settings: &Settings, accounts: &Accounts, to: KeyStorage) -> Result<Migration, AppError> {
    let mut names = settings.accounts.clone();
    let mut key_salt = settings.key_salt.clone();
    let mut stale = Vec::new();
    for source in Source::ALL {
        let (Some(saved), Some(key)) = (names.get_mut(source), accounts.api_key(source)) else { continue };
        match to {
            KeyStorage::File => {
                if saved.sealed_key.is_none() {
                    stale.push((source, saved.name.clone()));
                }
                let salt = match &key_salt {
                    Some(salt) => salt.clone(),
                    None => key_salt.insert(sealed::new_salt()?).clone(),
                };
                saved.sealed_key = Some(sealed::seal(key, &salt, &context(source, &saved.name))?);
            }
            KeyStorage::Keychain => {
                secrets::write(source, &saved.name, key)?;
                saved.sealed_key = None;
            }
        }
    }
    Ok(Migration { accounts: names, key_salt, stale })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_storage_round_trips_through_settings() {
        let mut settings = Settings { key_storage: KeyStorage::File, ..Settings::default() };
        let sealed = store(&mut settings, Source::Gelbooru, "42", "secret").unwrap();
        assert!(settings.key_salt.is_some());
        settings.accounts.gelbooru = Some(SavedAccount { name: "42".into(), level: None, sealed_key: sealed });
        let (accounts, error) = load(&settings);
        assert_eq!(error, None);
        assert_eq!(accounts.api_key(Source::Gelbooru), Some("secret"));
        assert_eq!(accounts.api_key(Source::Danbooru), None);
    }

    #[test]
    fn keychain_to_file_migration_seals_keys_and_lists_stale_entries() {
        let settings = Settings {
            accounts: AccountNames {
                danbooru: Some(SavedAccount { name: "sora".into(), level: None, sealed_key: None }),
                gelbooru: Some(SavedAccount { name: "42".into(), level: None, sealed_key: None }),
                e621: None,
                rule34: None,
                pixiv: None,
            },
            ..Settings::default()
        };
        let mut accounts = Accounts::default();
        accounts.set(Source::Danbooru, Some(("sora".into(), "d-key".into())));
        // Gelbooru 的 Key 读不到（需要重新填写），保持原样。
        let migration = migrate(&settings, &accounts, KeyStorage::File).unwrap();
        assert_eq!(migration.stale, vec![(Source::Danbooru, "sora".to_string())]);
        assert!(migration.accounts.danbooru.as_ref().unwrap().sealed_key.is_some());
        assert!(migration.accounts.gelbooru.as_ref().unwrap().sealed_key.is_none());

        let migrated = Settings {
            accounts: migration.accounts,
            key_salt: migration.key_salt,
            key_storage: KeyStorage::File,
            ..Settings::default()
        };
        let (loaded, _) = load(&Settings { accounts: AccountNames { gelbooru: None, ..migrated.accounts.clone() }, ..migrated });
        assert_eq!(loaded.api_key(Source::Danbooru), Some("d-key"));
    }
}
