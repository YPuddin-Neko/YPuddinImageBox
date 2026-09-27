use serde::Serialize;

/// 返回给前端的错误。序列化为 `{ code, message }`，界面按 code 决定怎么提示。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("网络请求失败：{0}")]
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
