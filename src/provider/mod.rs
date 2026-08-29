//! Wallpaper Provider 框架核心接口。
//!
//! 这是方案 §36 要求从第一版开始冻结的接口：
//! 核心程序只依赖 `Wallpaper` 与 `WallpaperProvider`，不感知任何数据源的 JSON 结构。

pub mod bing;

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

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("网络请求失败: {0}")]
    Network(#[from] reqwest::Error),
    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("API 未返回任何壁纸")]
    Empty,
}

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
