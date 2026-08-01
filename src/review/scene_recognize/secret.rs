//! 视觉 API Key 安全存储：系统钥匙串优先，环境变量仅作本地回退。
//!
//! - 不写 `scene_recognize_config.json`
//! - Host RPC 只接受写入/清除，从不在响应中回传明文
//! - macOS 使用 Keychain（`security`）；进程内缓存，避免每张图都弹授权

use std::process::Command;
use std::sync::Mutex;

use crate::review::error::{ReviewError, ReviewResult};
use crate::review::scene_recognize::config::VISION_API_KEY_ENV;

const KEYCHAIN_SERVICE: &str = "imgforge.vision.api_key";

#[derive(Clone)]
struct ResolvedKey {
    value: String,
    from_keychain: bool,
}

static CACHE: Mutex<Option<ResolvedKey>> = Mutex::new(None);

fn keychain_account() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "imgforge".into())
}

fn cache_get() -> Option<ResolvedKey> {
    CACHE.lock().ok().and_then(|g| g.clone())
}

fn cache_set(key: Option<ResolvedKey>) {
    if let Ok(mut g) = CACHE.lock() {
        *g = key;
    }
}

/// 仅查钥匙串是否有该项（不取密文，通常不弹密码框）。
#[cfg(target_os = "macos")]
fn keychain_exists() -> bool {
    Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            &keychain_account(),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn keychain_exists() -> bool {
    false
}

/// 从钥匙串读取密文（仅 macOS）。首次可能弹授权；之后走进程缓存。
#[cfg(target_os = "macos")]
fn keychain_load() -> Option<String> {
    let out = Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            &keychain_account(),
            "-w",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let key = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

#[cfg(not(target_os = "macos"))]
fn keychain_load() -> Option<String> {
    None
}

/// 写入钥匙串（覆盖已有项）。
/// `-A`：允许本机应用访问，避免识别时每张图弹登录密码。
#[cfg(target_os = "macos")]
fn keychain_store(key: &str) -> ReviewResult<()> {
    let account = keychain_account();
    let _ = Command::new("security")
        .args([
            "delete-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            &account,
        ])
        .output();

    let mut args = vec![
        "add-generic-password".into(),
        "-s".into(),
        KEYCHAIN_SERVICE.into(),
        "-a".into(),
        account,
        "-w".into(),
        key.to_string(),
        "-A".into(), // allow applications (避免识别时反复弹登录密码)
    ];
    // 额外把当前 host 加入 ACL
    if let Ok(exe) = std::env::current_exe() {
        args.push("-T".into());
        args.push(exe.to_string_lossy().into_owned());
    }
    args.push("-T".into());
    args.push("/usr/bin/security".into());

    let out = Command::new("security")
        .args(&args)
        .output()
        .map_err(|e| ReviewError::Message(format!("无法调用 Keychain：{e}")))?;
    if !out.status.success() {
        // 若 -A 与 -T 组合不被接受，回退仅 -A
        let account = keychain_account();
        let _ = Command::new("security")
            .args([
                "delete-generic-password",
                "-s",
                KEYCHAIN_SERVICE,
                "-a",
                &account,
            ])
            .output();
        let out2 = Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                KEYCHAIN_SERVICE,
                "-a",
                &account,
                "-w",
                key,
                "-A",
            ])
            .output()
            .map_err(|e| ReviewError::Message(format!("无法调用 Keychain：{e}")))?;
        if !out2.status.success() {
            let err = String::from_utf8_lossy(&out2.stderr);
            return Err(ReviewError::Message(format!(
                "写入 Keychain 失败：{}",
                err.trim()
            )));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn keychain_store(_key: &str) -> ReviewResult<()> {
    Err(ReviewError::Message(
        "当前平台不支持钥匙串存储，请使用环境变量 IMGFORGE_VISION_API_KEY".into(),
    ))
}

#[cfg(target_os = "macos")]
fn keychain_delete() -> ReviewResult<()> {
    let account = keychain_account();
    let out = Command::new("security")
        .args([
            "delete-generic-password",
            "-s",
            KEYCHAIN_SERVICE,
            "-a",
            &account,
        ])
        .output()
        .map_err(|e| ReviewError::Message(format!("无法调用 Keychain：{e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("could not be found")
            || err.contains("The specified item could not be found")
            || !keychain_exists()
        {
            return Ok(());
        }
        return Err(ReviewError::Message(format!(
            "删除 Keychain 项失败：{}",
            err.trim()
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn keychain_delete() -> ReviewResult<()> {
    Ok(())
}

fn env_load() -> Option<String> {
    std::env::var(VISION_API_KEY_ENV)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 解析 API Key：进程缓存 → 钥匙串 → 环境变量。
pub fn resolve_api_key() -> Option<String> {
    if let Some(c) = cache_get() {
        return Some(c.value);
    }
    if let Some(k) = keychain_load() {
        cache_set(Some(ResolvedKey {
            value: k.clone(),
            from_keychain: true,
        }));
        return Some(k);
    }
    if let Some(k) = env_load() {
        cache_set(Some(ResolvedKey {
            value: k.clone(),
            from_keychain: false,
        }));
        return Some(k);
    }
    None
}

pub fn has_api_key() -> bool {
    if cache_get().is_some() {
        return true;
    }
    keychain_exists() || env_load().is_some()
}

/// 是否已存在钥匙串条目（不含 env；不取密文）。
pub fn has_keychain_api_key() -> bool {
    if let Some(c) = cache_get() {
        if c.from_keychain {
            return true;
        }
    }
    keychain_exists()
}

pub fn store_api_key(key: &str) -> ReviewResult<()> {
    let key = key.trim();
    if key.is_empty() {
        return Err(ReviewError::Message("API Key 不能为空".into()));
    }
    if key.len() < 8 {
        return Err(ReviewError::Message("API Key 过短，请检查是否粘贴完整".into()));
    }
    keychain_store(key)?;
    cache_set(Some(ResolvedKey {
        value: key.to_string(),
        from_keychain: true,
    }));
    Ok(())
}

pub fn clear_api_key() -> ReviewResult<()> {
    keychain_delete()?;
    cache_set(None);
    Ok(())
}

/// 从错误文本中抹去疑似 Key，避免 HTTP 回显泄露。
pub fn redact_secrets(text: &str) -> String {
    let mut out = text.to_string();
    // 只用缓存，避免错误路径再触发钥匙串弹窗
    if let Some(c) = cache_get() {
        if c.value.len() >= 8 {
            out = out.replace(&c.value, "***");
        }
    }
    regex_lite_sk(&out)
}

fn regex_lite_sk(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 3 <= bytes.len() && &bytes[i..i + 3] == b"sk-" {
            out.push_str("sk-***");
            i += 3;
            while i < bytes.len() {
                let c = bytes[i];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' {
                    i += 1;
                } else {
                    break;
                }
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::regex_lite_sk;

    #[test]
    fn redact_sk_token() {
        let s = "Authorization Bearer sk-abcDEF123.xyz error";
        assert_eq!(regex_lite_sk(s), "Authorization Bearer sk-*** error");
    }
}
