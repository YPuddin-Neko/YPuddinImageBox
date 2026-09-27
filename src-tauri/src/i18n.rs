//! 界面语言：设置里选跟随系统、中文或 English。
//!
//! Rust 这边生成的文字（错误信息、托盘菜单、系统通知、下载任务的标题和跳过原因）按当前语言生成，
//! 界面上的其余文字由前端翻译。已经写进数据库的任务标题和原因保持写入时的语言。

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// 设置里选的语言。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguageSetting {
    #[default]
    System,
    Zh,
    En,
}

/// 实际使用的语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Zh,
    En,
}

impl LanguageSetting {
    pub fn resolve(self) -> Language {
        match self {
            LanguageSetting::Zh => Language::Zh,
            LanguageSetting::En => Language::En,
            LanguageSetting::System => from_locale(sys_locale::get_locale().as_deref()),
        }
    }
}

/// 系统首选语言是中文（简体、繁体都算）时用中文，其他语言都用英文；读不到时用中文。
fn from_locale(locale: Option<&str>) -> Language {
    match locale {
        Some(locale) if !locale.to_ascii_lowercase().starts_with("zh") => Language::En,
        _ => Language::Zh,
    }
}

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn current() -> Language {
    if is_english() {
        Language::En
    } else {
        Language::Zh
    }
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

pub fn set(language: Language) {
    ENGLISH.store(language == Language::En, Ordering::Relaxed);
}

/// 按当前语言二选一，用于不带参数的固定文字。
pub fn text(zh: &'static str, en: &'static str) -> &'static str {
    if is_english() {
        en
    } else {
        zh
    }
}

/// 按当前语言生成文字，两种写法都和 `format!` 一样：`tr!("订阅「{title}」", "Subscription “{title}”")`。
macro_rules! tr {
    ($zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::is_english() {
            format!($en $(, $arg)*)
        } else {
            format!($zh $(, $arg)*)
        }
    };
}
pub(crate) use tr;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_locales_use_chinese() {
        assert_eq!(from_locale(Some("zh-Hans-CN")), Language::Zh);
        assert_eq!(from_locale(Some("zh-TW")), Language::Zh);
        assert_eq!(from_locale(Some("en-US")), Language::En);
        assert_eq!(from_locale(Some("ja-JP")), Language::En);
        assert_eq!(from_locale(None), Language::Zh);
    }
}
