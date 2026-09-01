//! Wallpaper Provider 框架核心接口。
//!
//! 这是方案 §36 要求从第一版开始冻结的接口：
//! 核心程序只依赖 `Wallpaper` 与 `WallpaperProvider`，不感知任何数据源的 JSON 结构。

pub mod bing;
pub mod json;
pub mod manifest;
pub mod repo;
pub mod url;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 统一壁纸元数据（方案 §5）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallpaper {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub image_url: String,
    pub thumbnail_url: Option<String>,
    pub copyright: Option<String>,
    pub source: String,
    pub published_at: Option<DateTime<Utc>>,
}

/// 远程数据（API/更新源）提供的 id 白名单：仅 ASCII 字母数字与 `._-`，
/// 且不含 `..`。id 会被拼入缓存/安装文件路径，必须杜绝路径遍历与任意覆盖。
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
        && !id.contains("..")
}

/// 日志/UI 回显不可信字符串时的截断与控制字符清洗
pub fn echo_untrusted(text: &str) -> String {
    let printable: String = text
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .take(48)
        .collect();
    if text.chars().count() > 48 {
        format!("{printable}…")
    } else {
        printable
    }
}

const MAX_ID_LEN: usize = 128;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),
    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("API 未返回任何壁纸")]
    Empty,
    #[error("响应数据超过大小上限")]
    TooLarge,
}

/// 限制响应体大小后解析 JSON：防止恶意 API 返回超大响应耗尽内存
pub async fn json_with_limit<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, ProviderError> {
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > MAX_JSON_BYTES {
            return Err(ProviderError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&body)?)
}

const MAX_JSON_BYTES: usize = 10 * 1024 * 1024;

/// Provider 执行上下文（P0 仅共享 HTTP 客户端）
pub struct ProviderContext {
    pub http: reqwest::Client,
}

/// 核心扩展接口（决策 #2：async 签名）
#[async_trait]
pub trait WallpaperProvider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    async fn fetch(&self, context: &ProviderContext) -> Result<Vec<Wallpaper>, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_ids_accepted() {
        assert!(is_safe_id("bing"));
        assert!(is_safe_id("OHR.SombreroGalaxy_ZH-CN1234567890"));
        assert!(is_safe_id("2026-08-31"));
        assert!(is_safe_id("a"));
    }

    #[test]
    fn traversal_and_separators_rejected() {
        assert!(!is_safe_id("..\\..\\Roaming\\x"));
        assert!(!is_safe_id("../../x"));
        // 含 .. 的任何形式都拒绝
        assert!(!is_safe_id(".."));
        assert!(!is_safe_id("x..y"));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id("a\\b"));
        assert!(!is_safe_id("a:b"));
        assert!(!is_safe_id("/abs"));
        assert!(!is_safe_id("C:\\x"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id("空格 id"));
        assert!(!is_safe_id(&"a".repeat(129)));
    }

    #[test]
    fn echo_untrusted_truncates_and_sanitizes() {
        assert_eq!(echo_untrusted("bing"), "bing");
        assert_eq!(echo_untrusted("a\nb"), "a?b");
        let long = "x".repeat(60);
        let echoed = echo_untrusted(&long);
        assert!(echoed.chars().count() <= 49);
    }
}
