use serde::Serialize;

use crate::i18n::tr;

/// 返回给前端的错误。序列化为 `{ code, message }`，界面按 code 决定怎么提示；message 按当前界面语言生成。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    Network(#[from] reqwest::Error),
    Http { site: &'static str, status: u16 },
    TagLimit { site: &'static str, limit: u32 },
    /// 站点自己返回的错误说明，原样显示。
    Upstream { site: &'static str, message: String },
    CredentialsMissing(&'static str),
    /// 看自己的收藏要先登录这个站点。
    FavoritesSignIn(&'static str),
    Parse { site: &'static str, detail: String },
    Storage(#[from] crate::storage::StorageError),
    Database(#[from] sqlx::Error),
    /// 用户填写的内容不对，消息直接给用户看。
    InvalidInput(String),
    BadCredentials { site: &'static str },
    Keychain(String),
    Internal(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            AppError::Network(err) => {
                let detail = crate::net::network_detail(err);
                tr!("网络请求失败：{detail}", "Network request failed: {detail}")
            }
            AppError::Http { site, status } => tr!("{site} 返回 HTTP {status}", "{site} returned HTTP {status}"),
            AppError::TagLimit { site, limit } => tr!(
                "{site} 一次最多搜索 {limit} 个 tag，排除项和 order: 也计入",
                "{site} allows at most {limit} tags per search, counting excluded tags and order:"
            ),
            AppError::Upstream { site, message } => tr!("{site}：{message}", "{site}: {message}"),
            // Pixiv 不登录也能用，只有 R-18 作品要登录。
            AppError::CredentialsMissing("Pixiv") => {
                tr!("看 Pixiv 的 R-18 作品需要先登录 Pixiv", "Sign in to Pixiv to see R-18 works")
            }
            AppError::CredentialsMissing(site) => {
                tr!("{site} 需要账号和 API Key", "{site} requires an account and an API key")
            }
            AppError::FavoritesSignIn(site) => tr!(
                "看 {site} 的收藏要先在「设置 → 账号」里登录 {site}",
                "Sign in to {site} in Settings → Accounts to see your favorites"
            ),
            AppError::Parse { site, detail } => {
                tr!("{site} 的响应无法解析：{detail}", "Couldn't read the response from {site}: {detail}")
            }
            AppError::Storage(err) => err.to_string(),
            AppError::Database(err) => tr!("图库数据库出错：{err}", "Library database error: {err}"),
            AppError::InvalidInput(message) | AppError::Internal(message) => message.clone(),
            AppError::BadCredentials { site } => {
                tr!("{site} 的账号或 API Key 不对", "The {site} account or API key is incorrect")
            }
            AppError::Keychain(detail) if cfg!(target_os = "windows") => {
                tr!("读写 Windows 凭据管理器失败：{detail}", "Couldn't access Windows Credential Manager: {detail}")
            }
            AppError::Keychain(detail) => {
                tr!("读写系统钥匙串失败：{detail}", "Couldn't access the system keychain: {detail}")
            }
        };
        f.write_str(&message)
    }
}

impl AppError {
    /// 网络或站点临时出了问题，过一会儿再试可能就好了；账号、条件不对的错误再试也没用。
    pub fn is_transient(&self) -> bool {
        matches!(self, AppError::Network(_) | AppError::Http { .. } | AppError::Parse { .. })
    }

    fn code(&self) -> &'static str {
        match self {
            AppError::Network(_) => "network",
            AppError::Http { .. } => "http",
            AppError::TagLimit { .. } => "tag_limit",
            AppError::Upstream { .. } => "upstream",
            // 界面按这个 code 显示「填写账号」「登录」按钮，两种情况处理一样。
            AppError::CredentialsMissing(_) | AppError::FavoritesSignIn(_) => "credentials_missing",
            AppError::Parse { .. } => "parse",
            AppError::Storage(_) => "storage",
            AppError::Database(_) => "database",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::BadCredentials { .. } => "bad_credentials",
            AppError::Keychain(_) => "keychain",
            AppError::Internal(_) => "internal",
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: String,
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ErrorBody { code: self.code(), message: self.to_string() }.serialize(serializer)
    }
}
