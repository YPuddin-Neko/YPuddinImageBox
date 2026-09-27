use serde::Serialize;

/// 返回给前端的错误。序列化为 `{ code, message }`，界面按 code 决定怎么提示。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("网络请求失败：{}", crate::net::network_detail(.0))]
    Network(#[from] reqwest::Error),
    #[error("{site} 返回 HTTP {status}")]
    Http { site: &'static str, status: u16 },
    #[error("{site} 一次最多搜索 {limit} 个 tag，排除项和 order: 也计入")]
    TagLimit { site: &'static str, limit: u32 },
    #[error("{site}：{message}")]
    Upstream { site: &'static str, message: String },
    #[error("{0} 需要账号和 API Key")]
    CredentialsMissing(&'static str),
    #[error("{site} 的响应无法解析：{detail}")]
    Parse { site: &'static str, detail: String },
    #[error("{0}")]
    Storage(#[from] crate::storage::StorageError),
    #[error("图库数据库出错：{0}")]
    Database(#[from] sqlx::Error),
    /// 用户填写的内容不对，消息直接给用户看。
    #[error("{0}")]
    InvalidInput(String),
    #[error("{site} 的账号或 API Key 不对")]
    BadCredentials { site: &'static str },
    #[error("读写系统钥匙串失败：{0}")]
    Keychain(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    fn code(&self) -> &'static str {
        match self {
            AppError::Network(_) => "network",
            AppError::Http { .. } => "http",
            AppError::TagLimit { .. } => "tag_limit",
            AppError::Upstream { .. } => "upstream",
            AppError::CredentialsMissing(_) => "credentials_missing",
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
