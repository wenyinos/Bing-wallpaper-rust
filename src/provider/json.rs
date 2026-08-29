//! 通用 JSON Provider（方案 §6.2）：声明式 JSON Pointer 字段映射。
//!
//! 只做 HTTP + JSON 解析 + 字段映射（方案 §25 安全边界：无任何代码执行）。

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::manifest::ProviderManifest;
use super::{ProviderContext, ProviderError, Wallpaper, WallpaperProvider};

pub struct JsonProvider {
    manifest: Arc<ProviderManifest>,
}

impl JsonProvider {
    pub fn new(manifest: Arc<ProviderManifest>) -> Result<Self, String> {
        if manifest.endpoint.is_empty() {
            return Err(format!("JSON Provider '{}' 缺少 endpoint", manifest.id));
        }
        let mapping = manifest
            .mapping
            .as_ref()
            .ok_or_else(|| format!("JSON Provider '{}' 缺少 mapping", manifest.id))?;
        if mapping.image_url.is_empty() {
            return Err(format!(
                "JSON Provider '{}' 的 mapping 缺少 imageUrl",
                manifest.id
            ));
        }
        Ok(Self { manifest })
    }
}

#[async_trait]
impl WallpaperProvider for JsonProvider {
    fn id(&self) -> &str {
        &self.manifest.id
    }

    fn name(&self) -> &str {
        &self.manifest.name
    }

    async fn fetch(&self, context: &ProviderContext) -> Result<Vec<Wallpaper>, ProviderError> {
        let mut request = context.http.get(&self.manifest.endpoint);
        for (key, value) in &self.manifest.params {
            request = request.query(&[(key, value)]);
        }
        for (key, value) in &self.manifest.headers {
            request = request.header(key, value);
        }
        let value: serde_json::Value = request.send().await?.json().await?;

        let items = match &self.manifest.items_pointer {
            Some(pointer) => value.pointer(pointer).ok_or(ProviderError::Empty)?,
            None => &value,
        };
        let array = items.as_array().ok_or(ProviderError::Empty)?;
        let mapping = self.manifest.mapping.as_ref().ok_or(ProviderError::Empty)?;

        Ok(array
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let image_url = get_string(item, &mapping.image_url)?;
                let id = mapping
                    .id
                    .as_deref()
                    .and_then(|pointer| get_string(item, pointer))
                    .unwrap_or_else(|| index.to_string());
                let title = mapping
                    .title
                    .as_deref()
                    .and_then(|pointer| get_string(item, pointer))
                    .unwrap_or_default();
                let thumbnail_url = mapping
                    .thumbnail_url
                    .as_deref()
                    .and_then(|pointer| get_string(item, pointer));
                let copyright = mapping
                    .copyright
                    .as_deref()
                    .and_then(|pointer| get_string(item, pointer));
                let published_at = mapping
                    .published_at
                    .as_deref()
                    .and_then(|pointer| get_string(item, pointer))
                    .and_then(parse_datetime);
                Some(Wallpaper {
                    id,
                    title,
                    description: None,
                    image_url,
                    thumbnail_url,
                    copyright,
                    source: self.manifest.id.clone(),
                    published_at,
                })
            })
            .collect())
    }
}

/// 相对当前条目的 JSON Pointer（"" 表示条目本身；"id" 归一化为 "/id"）
fn get_string(item: &serde_json::Value, pointer: &str) -> Option<String> {
    let normalized = if pointer.is_empty() {
        String::new()
    } else if let Some(stripped) = pointer.strip_prefix('/') {
        format!("/{stripped}")
    } else {
        format!("/{pointer}")
    };
    match item.pointer(&normalized)? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_datetime(text: String) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(&text) {
        return Some(dt.with_timezone(&Utc));
    }
    chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc())
}
