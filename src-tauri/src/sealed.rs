//! 「加密保存在设置文件」用的加解密。
//!
//! 密钥由这台电脑的设备标识和第一次加密时生成的随机盐派生（HKDF-SHA256），
//! 用 XChaCha20-Poly1305 加密，并把「站点:用户名」作为附加数据绑定进去，
//! 密文不能挪给别的账号用。设置文件被单独复制到别处时解不开，换电脑需要重新填写。
//! 它挡不住在这台电脑上以同一用户运行的程序，那种情况请用系统钥匙串。

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::error::AppError;
use crate::i18n::tr;

const VERSION: &str = "v1.";
const INFO: &[u8] = b"com.ypuddin.imagebox api-key v1";
const NONCE_LEN: usize = 24;

fn random<const N: usize>() -> Result<[u8; N], AppError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes)
        .map_err(|e| AppError::Internal(tr!("无法生成随机数：{e}", "Couldn't generate random bytes: {e}")))?;
    Ok(bytes)
}

/// 新的随机盐，存在设置文件里。
pub fn new_salt() -> Result<String, AppError> {
    Ok(URL_SAFE_NO_PAD.encode(random::<16>()?))
}

fn cipher(salt: &str, machine_id: &str) -> Result<XChaCha20Poly1305, AppError> {
    let salt = URL_SAFE_NO_PAD.decode(salt).map_err(|_| {
        AppError::Internal(tr!("设置文件里的加密盐已损坏", "The encryption salt in the settings file is damaged"))
    })?;
    let derive_failed = || AppError::Internal(tr!("派生密钥失败", "Couldn't derive the encryption key"));
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&salt), machine_id.as_bytes()).expand(INFO, &mut key).map_err(|_| derive_failed())?;
    XChaCha20Poly1305::new_from_slice(&key).map_err(|_| derive_failed())
}

fn machine_id() -> Result<String, AppError> {
    machine_uid::get().map_err(|e| AppError::Internal(tr!("读取设备标识失败：{e}", "Couldn't read the device ID: {e}")))
}

/// 加密 API Key。`context` 是「站点:用户名」。
pub fn seal(plain: &str, salt: &str, context: &str) -> Result<String, AppError> {
    seal_with(plain, salt, context, &machine_id()?)
}

/// 解密；换了电脑、盐或账号不对时失败。
pub fn open(sealed: &str, salt: &str, context: &str) -> Result<String, AppError> {
    open_with(sealed, salt, context, &machine_id()?)
}

fn seal_with(plain: &str, salt: &str, context: &str, machine_id: &str) -> Result<String, AppError> {
    let nonce_bytes = random::<NONCE_LEN>()?;
    let nonce = XNonce::from(nonce_bytes);
    let payload = Payload { msg: plain.as_bytes(), aad: context.as_bytes() };
    let sealed = cipher(salt, machine_id)?
        .encrypt(&nonce, payload)
        .map_err(|_| AppError::Internal(tr!("加密失败", "Encryption failed")))?;
    let mut bytes = nonce_bytes.to_vec();
    bytes.extend_from_slice(&sealed);
    Ok(format!("{VERSION}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

fn open_with(sealed: &str, salt: &str, context: &str, machine_id: &str) -> Result<String, AppError> {
    let unreadable = || {
        AppError::Internal(tr!(
            "设置文件里的 API Key 无法解密（可能是从别的电脑复制来的），请重新填写",
            "The API key in the settings file can't be decrypted (it may have been copied from another computer). \
             Enter it again"
        ))
    };
    let bytes = sealed.strip_prefix(VERSION).and_then(|b| URL_SAFE_NO_PAD.decode(b).ok()).ok_or_else(unreadable)?;
    if bytes.len() <= NONCE_LEN {
        return Err(unreadable());
    }
    let (nonce, ciphertext) = bytes.split_at(NONCE_LEN);
    let nonce = XNonce::try_from(nonce).map_err(|_| unreadable())?;
    let payload = Payload { msg: ciphertext, aad: context.as_bytes() };
    let plain = cipher(salt, machine_id)?.decrypt(&nonce, payload).map_err(|_| unreadable())?;
    String::from_utf8(plain).map_err(|_| unreadable())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_on_the_same_machine() {
        let salt = new_salt().unwrap();
        let sealed = seal_with("my-api-key", &salt, "danbooru:sora", "machine-a").unwrap();
        assert!(sealed.starts_with(VERSION));
        assert!(!sealed.contains("my-api-key"));
        assert_eq!(open_with(&sealed, &salt, "danbooru:sora", "machine-a").unwrap(), "my-api-key");
        // 每次加密用新的随机数，同一个 Key 密文也不同。
        assert_ne!(sealed, seal_with("my-api-key", &salt, "danbooru:sora", "machine-a").unwrap());
    }

    #[test]
    fn fails_on_other_machine_salt_or_account() {
        let salt = new_salt().unwrap();
        let sealed = seal_with("my-api-key", &salt, "danbooru:sora", "machine-a").unwrap();
        assert!(open_with(&sealed, &salt, "danbooru:sora", "machine-b").is_err());
        assert!(open_with(&sealed, &new_salt().unwrap(), "danbooru:sora", "machine-a").is_err());
        assert!(open_with(&sealed, &salt, "gelbooru:sora", "machine-a").is_err());
        assert!(open_with("v1.garbage", &salt, "danbooru:sora", "machine-a").is_err());
        assert!(open_with("plain-text", &salt, "danbooru:sora", "machine-a").is_err());
    }

    #[test]
    fn real_machine_id_is_available() {
        let salt = new_salt().unwrap();
        let sealed = seal("key", &salt, "gelbooru:1").unwrap();
        assert_eq!(open(&sealed, &salt, "gelbooru:1").unwrap(), "key");
    }
}
