//! Provider Manifest（方案 §8/§24）：声明式数据源描述。
//!
//! 数据源接口变化时优先更新 Manifest 文件而不是重编译核心（方案 §24）。
//! 安全边界（方案 §25）：Manifest 只含 HTTP/JSON/映射数据，无任何代码执行能力。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use super::bing::BingProvider;
use super::json::JsonProvider;
use super::url::UrlProvider;
use super::WallpaperProvider;

pub const KIND_BING: &str = "bing";
pub const KIND_JSON: &str = "json";
pub const KIND_URL: &str = "url";

fn default_version() -> String {
    "1".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderManifest {
    pub id: String,
    #[serde(default = "default_version")]
    pub version: String,
    pub name: String,
    /// bing | json | url
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub params: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    /// JSON Provider：壁纸数组所在的 JSON Pointer（如 "/data"）
    #[serde(rename = "itemsPointer", default)]
    pub items_pointer: Option<String>,
    /// JSON Provider：字段映射
    #[serde(default)]
    pub mapping: Option<FieldMapping>,
    /// URL Provider：图片地址
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMapping {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "imageUrl", default)]
    pub image_url: String,
    #[serde(rename = "thumbnailUrl", default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub copyright: Option<String>,
    /// 支持 RFC3339 或 YYYY-MM-DD
    #[serde(rename = "publishedAt", default)]
    pub published_at: Option<String>,
}

pub struct LoadedProvider {
    pub manifest: ProviderManifest,
    pub provider: Arc<dyn WallpaperProvider>,
}

/// 内置 Bing Manifest（与 providers/bing.json 等效；用户目录同名 id 可覆盖）
pub fn builtin() -> ProviderManifest {
    let mut params = std::collections::BTreeMap::new();
    params.insert("mkt".into(), "zh-CN".into());
    ProviderManifest {
        id: "bing".into(),
        version: "2026.08.29".into(),
        name: "Bing".into(),
        kind: KIND_BING.into(),
        endpoint: String::new(),
        params,
        headers: Default::default(),
        items_pointer: None,
        mapping: None,
        url: None,
    }
}

pub fn build(manifest: ProviderManifest) -> Result<LoadedProvider, String> {
    let manifest = Arc::new(manifest);
    let provider: Arc<dyn WallpaperProvider> = match manifest.kind.as_str() {
        KIND_BING => Arc::new(BingProvider::from_manifest(&manifest)),
        KIND_JSON => Arc::new(JsonProvider::new(manifest.clone())?),
        KIND_URL => Arc::new(UrlProvider::new(manifest.clone())?),
        other => return Err(format!("未知 Provider 类型: {other}")),
    };
    Ok(LoadedProvider {
        manifest: (*manifest).clone(),
        provider,
    })
}

/// 用户 Manifest 目录：%LOCALAPPDATA%\BingWallpaper-Rust\providers\
pub fn user_providers_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("providers")
}

/// 加载全部 Provider：内置 + 用户目录（同 id 用户清单覆盖内置）。
/// 单个 Manifest 无效只跳过并告警，不影响其他 Provider（离线策略，方案 §17）。
pub fn load_all(data_dir: &Path) -> Vec<LoadedProvider> {
    let mut manifests = vec![builtin()];
    let dir = user_providers_dir(data_dir);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|ext| ext != "json").unwrap_or(true) {
                continue;
            }
            let loaded = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| {
                    serde_json::from_str::<ProviderManifest>(&text).map_err(|e| e.to_string())
                });
            match loaded {
                Ok(m) => {
                    info!("加载 Provider Manifest: {}（{}）", m.id, path.display());
                    manifests.retain(|existing| existing.id != m.id);
                    manifests.push(m);
                }
                Err(err) => warn!("跳过无效 Manifest {}: {err}", path.display()),
            }
        }
    }

    manifests
        .into_iter()
        .filter_map(|m| match build(m) {
            Ok(loaded) => Some(loaded),
            Err(err) => {
                warn!("Provider 构建失败: {err}");
                None
            }
        })
        .collect()
}
