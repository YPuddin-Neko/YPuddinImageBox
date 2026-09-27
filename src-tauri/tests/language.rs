//! 切换界面语言后，Rust 这边生成的文字跟着换。
//! 语言是全局设置，放进库的单元测试会和断言中文文字的测试互相干扰，所以单独编成一个测试程序。

use imagebox_lib::error::AppError;
use imagebox_lib::i18n::{self, Language};
use imagebox_lib::settings::parse_proxy_url;
use imagebox_lib::storage::StorageError;

#[test]
fn messages_follow_the_language() {
    i18n::set(Language::En);
    assert_eq!(i18n::current(), Language::En);
    assert_eq!(AppError::BadCredentials { site: "Danbooru" }.to_string(), "The Danbooru account or API key is incorrect");
    assert_eq!(
        AppError::Storage(StorageError::Same).to_string(),
        "The new location is the same as the current one"
    );
    assert_eq!(parse_proxy_url("").unwrap_err().to_string(), "Enter a proxy address, e.g. http://127.0.0.1:7890");

    i18n::set(Language::Zh);
    assert_eq!(AppError::BadCredentials { site: "Danbooru" }.to_string(), "Danbooru 的账号或 API Key 不对");
    assert_eq!(parse_proxy_url("").unwrap_err().to_string(), "代理地址不能为空，例如 http://127.0.0.1:7890");
}
