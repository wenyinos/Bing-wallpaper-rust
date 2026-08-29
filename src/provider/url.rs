//! URL Provider（方案 §7）：最简单的 Provider，固定图片地址每日一张。

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Local, TimeZone, Utc};

use super::manifest::ProviderManifest;
use super::{ProviderContext, ProviderError, Wallpaper, WallpaperProvider};

pub struct UrlProvider {
    manifest: Arc<ProviderManifest>,
}

impl UrlProvider {
    pub fn new(manifest: Arc<ProviderManifest>) -> Result<Self, String> {
        if manifest.url.as_deref().unwrap_or("").is_empty() {
            return Err(format!("URL Provider '{}' 缺少 url 字段", manifest.id));
        }
        Ok(Self { manifest })
    }
}

#[async_trait]
impl WallpaperProvider for UrlProvider {
    fn id(&self) -> &str {
        &self.manifest.id
    }

    fn name(&self) -> &str {
        &self.manifest.name
    }

    async fn fetch(&self, _context: &ProviderContext) -> Result<Vec<Wallpaper>, ProviderError> {
        let url = self.manifest.url.clone().unwrap_or_default();
        let today = Local::now().date_naive();
        // id 按日变化：天然支持"每日一图"的缓存与日期驱动调度
        let id = format!("url-{}", today.format("%Y%m%d"));
        let published_at = Utc
            .from_local_datetime(&today.and_hms_opt(0, 0, 0).expect("midnight is valid"))
            .single();
        Ok(vec![Wallpaper {
            id,
            title: self.manifest.name.clone(),
            description: None,
            image_url: url,
            thumbnail_url: None,
            copyright: None,
            source: self.manifest.id.clone(),
            published_at,
        }])
    }
}
