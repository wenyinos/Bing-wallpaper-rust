//! Bing Provider：HPImageArchive API（双预设可切换，见方案 §6.1 与决策 #9）。

use async_trait::async_trait;
use serde::Deserialize;

use super::{ProviderContext, ProviderError, Wallpaper, WallpaperProvider};

pub struct BingProvider {
    /// 站点根地址，形如 `https://cn.bing.com`
    endpoint: String,
    mkt: String,
}

impl BingProvider {
    /// 由 Manifest 构建（方案 §8：Bing 接口变化优先改 providers/bing.json）
    pub fn from_manifest(manifest: &super::manifest::ProviderManifest) -> Self {
        let mkt = manifest
            .params
            .get("mkt")
            .cloned()
            .unwrap_or_else(|| "zh-CN".into());
        let endpoint = if manifest.endpoint.is_empty() {
            if mkt.starts_with("zh") {
                "https://cn.bing.com"
            } else {
                "https://www.bing.com"
            }
            .into()
        } else {
            manifest.endpoint.clone()
        };
        Self { endpoint, mkt }
    }
}

#[derive(Debug, Deserialize)]
struct HpImageArchive {
    #[serde(default)]
    images: Vec<HpImage>,
}

#[derive(Debug, Deserialize)]
struct HpImage {
    #[serde(default)]
    startdate: String,
    /// 形如 `/th?id=OHR.xxx_1920x1080.jpg&rf=...&pid=hp`，站内相对路径
    #[serde(default)]
    url: String,
    #[serde(default)]
    urlbase: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    copyright: Option<String>,
}

#[async_trait]
impl WallpaperProvider for BingProvider {
    fn id(&self) -> &str {
        "bing"
    }

    fn name(&self) -> &str {
        "Bing"
    }

    async fn fetch(&self, context: &ProviderContext) -> Result<Vec<Wallpaper>, ProviderError> {
        // idx=0 从今天起，n=8 为 API 单次上限（方案 §6.1）
        let url = format!(
            "{}/HPImageArchive.aspx?format=js&idx=0&n=8&mkt={}",
            self.endpoint, self.mkt
        );
        let archive: HpImageArchive = context.http.get(&url).send().await?.json().await?;
        if archive.images.is_empty() {
            return Err(ProviderError::Empty);
        }

        Ok(archive
            .images
            .into_iter()
            .filter_map(|img| {
                if img.url.is_empty() {
                    return None;
                }
                let image_url = format!("{}{}", self.endpoint, img.url);
                let id = if img.urlbase.is_empty() {
                    img.startdate.clone()
                } else {
                    img.urlbase
                        .trim_start_matches("/th?id=")
                        .trim_end_matches("_1920x1080")
                        .to_string()
                };
                let published_at = chrono::NaiveDate::parse_from_str(&img.startdate, "%Y%m%d")
                    .ok()
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .map(|dt| dt.and_utc());
                Some(Wallpaper {
                    id,
                    title: img.title.unwrap_or_default(),
                    description: None,
                    image_url,
                    thumbnail_url: None,
                    copyright: img.copyright,
                    source: "bing".into(),
                    published_at,
                })
            })
            .collect())
    }
}
